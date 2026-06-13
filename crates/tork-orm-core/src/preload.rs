//! Preloading: loading a relation's rows without the N+1 problem.
//!
//! A plain [`join`](crate::QuerySet::join) filters but does not load related rows.
//! Preloading runs the parent query, then a single follow-up query per relation
//! that loads every related row in one `WHERE key IN (...)`, and stitches the
//! results onto each parent. The parents come back wrapped in [`Preloaded`], which
//! derefs to the parent and exposes the related rows by type.

use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::marker::PhantomData;
use std::ops::Deref;
use std::pin::Pin;

use crate::dialect::{render_select, Dialect};
use crate::executor::Executor;
use crate::model::Model;
use crate::query::ast::{SelectItem, SelectStatement};
use crate::query::expr::Expr;
use crate::query::QuerySet;
use crate::relation::Relation;
use crate::row::Row;
use crate::value::Value;

/// A boxed, `Send` future borrowing for `'a`.
type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// An object-safe view of an [`Executor`], so a preload plan can run a query
/// without being generic over the executor type.
trait QueryRunner: Sync {
    fn dialect(&self) -> &dyn Dialect;
    fn fetch_all<'a>(
        &'a self,
        sql: String,
        params: Vec<Value>,
    ) -> BoxFuture<'a, crate::Result<Vec<Row>>>;
}

impl<E: Executor + Sync> QueryRunner for E {
    fn dialect(&self) -> &dyn Dialect {
        Executor::dialect(self)
    }

    fn fetch_all<'a>(
        &'a self,
        sql: String,
        params: Vec<Value>,
    ) -> BoxFuture<'a, crate::Result<Vec<Row>>> {
        Box::pin(Executor::fetch_all(self, sql, params))
    }
}

/// A parent paired with the related rows preloaded for it.
///
/// Derefs to the parent, so all of the model's fields are available directly;
/// [`get`](Preloaded::get) returns the related rows of a given type.
///
/// # Examples
///
/// ```no_run
/// # use tork_orm_core::preload::Preloaded;
/// # struct User { id: i64 }
/// # struct Post;
/// # fn show(user: &Preloaded<User>) {
/// let posts: &[Post] = user.get::<Post>();
/// let id = user.id; // deref to the parent
/// # let _ = (posts, id);
/// # }
/// ```
/// Identifies one preloaded relation on a parent.
///
/// Keyed by the related type plus the join columns so that a parent with two
/// relations to the *same* child type (for example `author` and `reviewer`,
/// both `User`) keeps a separate slot for each instead of one silently
/// overwriting the other.
#[derive(Clone, PartialEq, Eq, Hash)]
struct RelationKey {
    type_id: TypeId,
    from_column: &'static str,
    to_column: &'static str,
}

impl RelationKey {
    fn of<P, C: 'static>(relation: &Relation<P, C>) -> Self {
        Self {
            type_id: TypeId::of::<C>(),
            from_column: relation.from_column(),
            to_column: relation.to_column(),
        }
    }
}

pub struct Preloaded<M> {
    parent: M,
    relations: HashMap<RelationKey, Box<dyn Any + Send + Sync>>,
}

impl<M> Preloaded<M> {
    /// Returns the preloaded rows of type `C`, or an empty slice if none were
    /// loaded for this parent.
    ///
    /// When the parent has more than one relation to the same child type (for
    /// example `author` and `reviewer`, both `User`), this returns one of them;
    /// use [`get_via`](Preloaded::get_via) with the specific relation to select
    /// exactly which one.
    pub fn get<C: Send + Sync + 'static>(&self) -> &[C] {
        let type_id = TypeId::of::<C>();
        self.relations
            .iter()
            .find(|(key, _)| key.type_id == type_id)
            .and_then(|(_, boxed)| boxed.downcast_ref::<Vec<C>>())
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Returns the preloaded rows for one specific relation.
    ///
    /// Unlike [`get`](Preloaded::get), this disambiguates between multiple
    /// relations that target the same child type by matching the relation's join
    /// columns, so `preloaded.get_via(Post::author())` and
    /// `preloaded.get_via(Post::reviewer())` return their own rows.
    pub fn get_via<C: Send + Sync + 'static>(&self, relation: &Relation<M, C>) -> &[C] {
        self.relations
            .get(&RelationKey::of(relation))
            .and_then(|boxed| boxed.downcast_ref::<Vec<C>>())
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Returns a reference to the parent.
    pub fn parent(&self) -> &M {
        &self.parent
    }

    /// Consumes the wrapper, returning the parent.
    pub fn into_parent(self) -> M {
        self.parent
    }
}

impl<M> Deref for Preloaded<M> {
    type Target = M;

    fn deref(&self) -> &M {
        &self.parent
    }
}

/// The result of running one preload plan over the parents.
struct PlanOutput {
    key: RelationKey,
    /// One boxed `Vec<C>` per parent, in the same order as the parents.
    per_parent: Vec<Box<dyn Any + Send + Sync>>,
}

/// A single preload step, erased over the related model type.
trait PreloadStep<M>: Send + Sync {
    fn load<'a>(
        &'a self,
        parents: &'a [M],
        runner: &'a dyn QueryRunner,
    ) -> BoxFuture<'a, crate::Result<PlanOutput>>;
}

/// A preload step for one relation from `M` to `C`.
struct RelationPreload<M, C> {
    relation: Relation<M, C>,
}

impl<M: Model, C: Model> PreloadStep<M> for RelationPreload<M, C> {
    fn load<'a>(
        &'a self,
        parents: &'a [M],
        runner: &'a dyn QueryRunner,
    ) -> BoxFuture<'a, crate::Result<PlanOutput>> {
        Box::pin(async move {
            let from_column = self.relation.from_column();
            let to_column = self.relation.to_column();

            // Collect the distinct parent keys to load children for.
            let mut keys: Vec<Value> = Vec::new();
            let mut seen: HashSet<String> = HashSet::new();
            for parent in parents {
                if let Some(value) = column_value(parent, from_column) {
                    if seen.insert(value_key(&value)) {
                        keys.push(value);
                    }
                }
            }

            let relation_key = RelationKey::of(&self.relation);

            // No keys means nothing to load; every parent gets an empty list.
            if keys.is_empty() {
                return Ok(PlanOutput {
                    key: relation_key,
                    per_parent: parents.iter().map(|_| empty_children::<C>()).collect(),
                });
            }

            // The IN (...) list binds one parameter per distinct key, which can
            // exceed the backend's bind-parameter ceiling for large parent sets
            // (SQLite's `too many SQL variables`). Split the keys into chunks that
            // fit `Dialect::max_bind_params`, running one query each and merging
            // the rows. A small margin leaves room for the relation's own preload
            // filters, which bind additional parameters.
            const FILTER_PARAM_MARGIN: usize = 16;
            let chunk_size = runner
                .dialect()
                .max_bind_params()
                .saturating_sub(FILTER_PARAM_MARGIN)
                .max(1);

            // Group the related rows by their join key, accumulated across chunks.
            let mut groups: HashMap<String, Vec<Row>> = HashMap::new();
            for key_chunk in keys.chunks(chunk_size) {
                let projection = C::COLUMNS
                    .iter()
                    .map(|column| SelectItem::Column {
                        table: C::TABLE,
                        column: column.name,
                    })
                    .collect();
                let mut statement = SelectStatement::new(C::TABLE, projection);
                statement.filters.push(Expr::in_list(
                    Expr::column(C::TABLE, to_column),
                    key_chunk.to_vec(),
                ));
                statement
                    .filters
                    .extend(self.relation.preload_filters().iter().cloned());
                statement
                    .order_by
                    .extend(self.relation.preload_order_by().iter().cloned());

                let (sql, params) = render_select(runner.dialect(), &statement);
                let rows = runner.fetch_all(sql, params).await?;
                for row in rows {
                    let key = value_key(&row.get::<Value>(to_column)?);
                    groups.entry(key).or_default().push(row);
                }
            }

            // Map each parent's group into instances, preserving parent order.
            let mut per_parent: Vec<Box<dyn Any + Send + Sync>> = Vec::with_capacity(parents.len());
            for parent in parents {
                let children: Vec<C> = match column_value(parent, from_column) {
                    Some(value) => match groups.get(&value_key(&value)) {
                        Some(rows) => rows
                            .iter()
                            .map(C::from_row)
                            .collect::<crate::Result<Vec<C>>>()?,
                        None => Vec::new(),
                    },
                    None => Vec::new(),
                };
                per_parent.push(Box::new(children));
            }

            Ok(PlanOutput {
                key: relation_key,
                per_parent,
            })
        })
    }
}

/// A query that preloads one or more relations.
///
/// Created by [`QuerySet::preload`]. The usual builder methods forward to the
/// underlying query, so filtering, ordering, and limiting still apply to the
/// parents.
pub struct Preloader<M: Model> {
    base: QuerySet<M>,
    plans: Vec<Box<dyn PreloadStep<M>>>,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Model> Preloader<M> {
    /// Wraps a base query.
    pub(crate) fn new(base: QuerySet<M>) -> Self {
        Self {
            base,
            plans: Vec::new(),
            _marker: PhantomData,
        }
    }

    /// Adds another relation to preload.
    pub fn preload<C: Model>(mut self, relation: Relation<M, C>) -> Self {
        self.plans.push(Box::new(RelationPreload { relation }));
        self
    }

    /// Adds a parent predicate joined with `AND`.
    pub fn filter(mut self, predicate: Expr) -> Self {
        self.base = self.base.filter(predicate);
        self
    }

    /// Adds an `AND (a OR b OR ...)` group over the parents.
    pub fn filter_any(mut self, predicates: impl IntoIterator<Item = Expr>) -> Self {
        self.base = self.base.filter_any(predicates);
        self
    }

    /// Adds an `AND (a AND b AND ...)` group over the parents.
    pub fn filter_all(mut self, predicates: impl IntoIterator<Item = Expr>) -> Self {
        self.base = self.base.filter_all(predicates);
        self
    }

    /// Adds an `AND NOT (...)` over the parents.
    pub fn filter_not(mut self, predicate: Expr) -> Self {
        self.base = self.base.filter_not(predicate);
        self
    }

    /// Orders the parents.
    pub fn order_by(mut self, term: crate::query::ast::OrderTerm) -> Self {
        self.base = self.base.order_by(term);
        self
    }

    /// Limits the number of parents.
    pub fn limit(mut self, limit: u64) -> Self {
        self.base = self.base.limit(limit);
        self
    }

    /// Skips leading parents.
    pub fn offset(mut self, offset: u64) -> Self {
        self.base = self.base.offset(offset);
        self
    }

    /// Returns only distinct parents.
    pub fn distinct(mut self) -> Self {
        self.base = self.base.distinct();
        self
    }

    /// Runs the query and preloads, returning each parent with its related rows.
    pub async fn all<E: Executor + Sync>(self, executor: E) -> crate::Result<Vec<Preloaded<M>>> {
        let parents = self.base.all(&executor).await?;
        let mut relation_maps: Vec<HashMap<RelationKey, Box<dyn Any + Send + Sync>>> =
            (0..parents.len()).map(|_| HashMap::new()).collect();

        for plan in &self.plans {
            let output = plan.load(&parents, &executor).await?;
            for (index, children) in output.per_parent.into_iter().enumerate() {
                relation_maps[index].insert(output.key.clone(), children);
            }
        }

        Ok(parents
            .into_iter()
            .zip(relation_maps)
            .map(|(parent, relations)| Preloaded { parent, relations })
            .collect())
    }

    /// Runs the query and returns the first parent with its related rows, if any.
    pub async fn first<E: Executor + Sync>(
        self,
        executor: E,
    ) -> crate::Result<Option<Preloaded<M>>> {
        Ok(self.limit(1).all(executor).await?.into_iter().next())
    }
}

/// Reads a model instance's value for `column`, whether it is the primary key or
/// an ordinary column.
fn column_value<M: Model>(model: &M, column: &str) -> Option<Value> {
    if column == M::PRIMARY_KEY {
        Some(model.primary_key_value())
    } else {
        model
            .insert_values()
            .into_iter()
            .find(|(name, _)| *name == column)
            .map(|(_, value)| value)
    }
}

/// Builds a stable string key for grouping by a value. Join keys are integers or
/// text in practice, so a tagged debug rendering is unambiguous.
fn value_key(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(b) => format!("b:{b}"),
        Value::Int(i) => format!("i:{i}"),
        Value::Real(r) => format!("r:{r}"),
        Value::Text(s) => format!("t:{s}"),
        Value::Blob(bytes) => format!("x:{bytes:?}"),
        Value::Timestamp(ts) => format!("ts:{ts:?}"),
        Value::Uuid(u) => format!("u:{u}"),
        Value::Json(j) => format!("j:{j}"),
        Value::Array(items) => format!("a:{items:?}"),
    }
}

/// An empty boxed child list for a parent with no related rows.
fn empty_children<C: Send + Sync + 'static>() -> Box<dyn Any + Send + Sync> {
    Box::new(Vec::<C>::new())
}
