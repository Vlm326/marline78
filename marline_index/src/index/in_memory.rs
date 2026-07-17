use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use crate::sketch::Sketch;

use super::error::IndexError;
use super::{Entry, EntryId, PutOutcome, SearchOptions, SketchKvindex};

pub(crate) struct InnerIndex<S: Send + Sync + Sketch, V: Send + Sync> {
    key_to_id: HashMap<S, EntryId>,
    entries: HashMap<EntryId, Entry<S, V>>,
    postings: HashMap<u32, HashSet<EntryId>>,
    active_ids: HashSet<EntryId>,
    next_entry_id: EntryId,
}

impl<S, V> InnerIndex<S, V>
where
    S: Sketch,
    V: Clone + Send + Sync,
{
    fn new() -> Self {
        Self {
            key_to_id: HashMap::new(),
            entries: HashMap::new(),
            postings: HashMap::new(),
            active_ids: HashSet::new(),
            next_entry_id: 0,
        }
    }

    fn insert_postings(&mut self, key: &S, entry_id: EntryId) {
        for &element in key.as_slice() {
            self.postings.entry(element).or_default().insert(entry_id);
        }
    }

    fn remove_postings(&mut self, key: &S, entry_id: EntryId) {
        for &element in key.as_slice() {
            if let Some(set) = self.postings.get_mut(&element) {
                set.remove(&entry_id);
                if set.is_empty() {
                    self.postings.remove(&element);
                }
            }
        }
    }

    pub(crate) fn insert_entry(
        &mut self,
        key: S,
        value: V,
    ) -> Result<EntryId, IndexError> {
        if self.key_to_id.contains_key(&key) {
            return Err(IndexError::InternalInvariantViolation);
        }

        let entry_id = self.next_entry_id;
        self.next_entry_id = self
            .next_entry_id
            .checked_add(1)
            .ok_or(IndexError::EntryIdExhausted)?;

        let entry = Entry { id: entry_id, key: key.clone(), value };
        self.entries.insert(entry_id, entry);
        self.active_ids.insert(entry_id);
        self.key_to_id.insert(key.clone(), entry_id);
        self.insert_postings(&key, entry_id);

        Ok(entry_id)
    }

    pub(crate) fn remove_entry(
        &mut self,
        id: EntryId,
    ) -> Result<Entry<S, V>, IndexError> {
        let entry = self.entries.remove(&id).ok_or(IndexError::InternalInvariantViolation)?;
        self.active_ids.remove(&id);
        self.key_to_id.remove(&entry.key);
        self.remove_postings(&entry.key, id);
        Ok(entry)
    }

    pub(crate) fn update_entry(
        &mut self,
        id: EntryId,
        value: V,
    ) -> Result<Entry<S, V>, IndexError> {
        let entry = self.entries.get_mut(&id).ok_or(IndexError::InternalInvariantViolation)?;
        let old_value = std::mem::replace(&mut entry.value, value);
        let old_entry = Entry { id: entry.id, key: entry.key.clone(), value: old_value };
        Ok(old_entry)
    }

    #[allow(dead_code)]
    pub(crate) fn lookup(&self, key: &S) -> Option<EntryId> {
        self.key_to_id.get(key).copied()
    }

    fn clear(&mut self) {
        self.key_to_id.clear();
        self.entries.clear();
        self.postings.clear();
        self.active_ids.clear();
        self.next_entry_id = 0;
    }
}

pub struct InMemorySketchIndex<S: Send + Sync + Sketch, V: Send + Sync> {
    inner: Arc<RwLock<InnerIndex<S, V>>>,
}

impl<S, V> InMemorySketchIndex<S, V>
where
    S: Sketch,
    V: Clone + Send + Sync,
{
    pub fn new() -> Self {
        Self { inner: Arc::new(RwLock::new(InnerIndex::new())) }
    }

    #[allow(dead_code)]
    pub(crate) fn inner(&self) -> &Arc<RwLock<InnerIndex<S, V>>> {
        &self.inner
    }
}

impl<S, V> Default for InMemorySketchIndex<S, V>
where
    S: Sketch,
    V: Clone + Send + Sync,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<S, V> SketchKvindex<S> for InMemorySketchIndex<S, V>
where
    S: Sketch,
    V: Clone + Send + Sync,
{
    type Value = V;
    type Error = IndexError;

    fn len(&self) -> Result<usize, IndexError> {
        let guard = self.inner.read().map_err(|_| IndexError::InternalInvariantViolation)?;
        Ok(guard.active_ids.len())
    }

    fn get(&self, key: &S) -> Result<Option<Arc<V>>, IndexError> {
        let guard = self.inner.read().map_err(|_| IndexError::InternalInvariantViolation)?;
        let id = guard.key_to_id.get(key).copied();
        match id {
            Some(id) => {
                let entry = guard.entries.get(&id).ok_or(IndexError::InternalInvariantViolation)?;
                Ok(Some(Arc::new(entry.value.clone())))
            }
            None => Ok(None),
        }
    }

    fn put(&self, key: &S, value: Arc<V>) -> Result<PutOutcome, IndexError> {
        let mut guard = self.inner.write().map_err(|_| IndexError::InternalInvariantViolation)?;

        if let Some(&existing_id) = guard.key_to_id.get(key) {
            let old = guard.update_entry(existing_id, (*value).clone())?;
            Ok(PutOutcome::Updated {
                entry_id: existing_id,
                previous_entry_id: old.id,
            })
        } else {
            let entry_id = guard.insert_entry(key.clone(), (*value).clone())?;
            Ok(PutOutcome::Inserted { entry_id })
        }
    }

    fn remove(&self, key: &S) -> Result<(), IndexError> {
        let mut guard = self.inner.write().map_err(|_| IndexError::InternalInvariantViolation)?;
        let id = guard.key_to_id.get(key).copied().ok_or(IndexError::InternalInvariantViolation)?;
        guard.remove_entry(id)?;
        Ok(())
    }

    fn nearest(
        &self,
        _key: &S,
        _search_options: SearchOptions,
    ) -> Result<Option<(Arc<V>, usize)>, IndexError> {
        todo!("similarity search is not implemented yet")
    }

    fn top_k(
        &self,
        _key: &S,
        _k: usize,
        _search_options: SearchOptions,
    ) -> Result<Vec<(Arc<V>, usize)>, IndexError> {
        todo!("similarity search is not implemented yet")
    }

    fn clear(&self) -> Result<(), IndexError> {
        let mut guard = self.inner.write().map_err(|_| IndexError::InternalInvariantViolation)?;
        guard.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::PutOutcome;
    use crate::sketch::FixedSketch;

    fn sk6(vals: [u32; 6]) -> FixedSketch<6> {
        FixedSketch::new(vals).unwrap()
    }

    #[test]
    fn insert_and_lookup() {
        let index = InMemorySketchIndex::<FixedSketch<6>, String>::new();
        let key = sk6([1, 2, 3, 4, 5, 6]);
        let outcome = index.put(&key, Arc::new("hello".to_string())).unwrap();
        match outcome {
            PutOutcome::Inserted { entry_id } => assert_eq!(entry_id, 0),
            _ => panic!("expected Inserted"),
        }
        assert_eq!(index.len().unwrap(), 1);
        let val = index.get(&key).unwrap().unwrap();
        assert_eq!(*val, "hello");
    }

    #[test]
    fn update_existing_key() {
        let index = InMemorySketchIndex::<FixedSketch<6>, String>::new();
        let key = sk6([1, 2, 3, 4, 5, 6]);
        index.put(&key, Arc::new("first".to_string())).unwrap();
        let outcome = index.put(&key, Arc::new("second".to_string())).unwrap();
        match outcome {
            PutOutcome::Updated { entry_id, previous_entry_id } => {
                assert_eq!(entry_id, 0);
                assert_eq!(previous_entry_id, 0);
            }
            _ => panic!("expected Updated"),
        }
        assert_eq!(index.len().unwrap(), 1);
        assert_eq!(*index.get(&key).unwrap().unwrap(), "second");
    }

    #[test]
    fn remove_entry() {
        let index = InMemorySketchIndex::<FixedSketch<6>, String>::new();
        let key = sk6([1, 2, 3, 4, 5, 6]);
        index.put(&key, Arc::new("hello".to_string())).unwrap();
        index.remove(&key).unwrap();
        assert_eq!(index.len().unwrap(), 0);
        assert!(index.get(&key).unwrap().is_none());
    }

    #[test]
    fn clear_index() {
        let index = InMemorySketchIndex::<FixedSketch<6>, String>::new();
        index.put(&sk6([1, 2, 3, 4, 5, 6]), Arc::new("a".to_string())).unwrap();
        index.put(&sk6([10, 20, 30, 40, 50, 60]), Arc::new("b".to_string())).unwrap();
        index.clear().unwrap();
        assert_eq!(index.len().unwrap(), 0);
    }

    #[test]
    fn lookup_returns_correct_id() {
        let mut inner = InnerIndex::<FixedSketch<6>, String>::new();
        let id0 = inner.insert_entry(sk6([1, 2, 3, 4, 5, 6]), "six".to_string()).unwrap();
        let id1 = inner.insert_entry(sk6([10, 20, 30, 40, 50, 60]), "sixty".to_string()).unwrap();
        assert_eq!(id0, 0);
        assert_eq!(id1, 1);
        assert_eq!(inner.lookup(&sk6([1, 2, 3, 4, 5, 6])), Some(0));
        assert_eq!(inner.lookup(&sk6([10, 20, 30, 40, 50, 60])), Some(1));
        assert_eq!(inner.lookup(&sk6([90, 91, 92, 93, 94, 95])), None);
    }

    #[test]
    fn insert_entry_duplicate_key_errors() {
        let mut inner = InnerIndex::<FixedSketch<6>, String>::new();
        inner.insert_entry(sk6([1, 2, 3, 4, 5, 6]), "first".to_string()).unwrap();
        let result = inner.insert_entry(sk6([1, 2, 3, 4, 5, 6]), "second".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn different_permutations_same_key() {
        let mut inner = InnerIndex::<FixedSketch<6>, String>::new();
        let a = inner.insert_entry(sk6([60, 10, 30, 20, 50, 40]), "a".to_string()).unwrap();
        let b = inner.insert_entry(sk6([40, 50, 60, 10, 20, 30]), "b".to_string());
        assert!(b.is_err());
        assert_eq!(inner.lookup(&sk6([10, 20, 30, 40, 50, 60])), Some(a));
    }
}
