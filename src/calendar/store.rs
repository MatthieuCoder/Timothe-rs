use std::{
    collections::{BTreeMap, HashMap},
    ops::Add,
};

use chrono::{Duration, NaiveDateTime};

pub enum UpdateResult {
    Created,
    Updated(CalendarEvent),
    Unchanged,
}

#[derive(Debug, Default, Eq, PartialEq, Clone)]
/// This struct is stored in disk and indexed by it's uid (from ADE)
/// We can simply diff the events using their uid.
pub struct CalendarEvent {
    /// Summary of the event (Title)
    pub summary: String,
    /// Start of the event. (Utc aligned according to the iCalendar spec)
    pub start: NaiveDateTime,
    /// End of the event. (Utc aligned according to the iCalendar spec)
    pub end: NaiveDateTime,
    /// Where the event takes place.
    pub location: String,
    /// Description of the event.
    pub description: String,
    /// Last modification of the event.
    pub last_modified: NaiveDateTime,
    /// Creation date of the event.
    pub created: NaiveDateTime,
    /// Unique id of the event.
    pub uid: String,
}

/// A calendar is a collection of events
/// and utility functions used to search and sort them.
pub struct Calendar<'a> {
    // used to easily compute using dates
    tree: BTreeMap<NaiveDateTime, &'a CalendarEvent>,
    // used to search based on uids
    uid_index: HashMap<String, CalendarEvent>,
}
impl<'a> Calendar<'a> {
    pub fn new<'b>() -> Calendar<'b> {
        Calendar {
            tree: BTreeMap::new(),
            uid_index: HashMap::new(),
        }
    }

    pub fn get_range(&self, date: NaiveDateTime, duration: Duration) -> Vec<&&CalendarEvent> {
        // get all the events using the tree map
        // this is fast because we just search the binary tree (=few comparaisons to get to the leaf node containing the pointer to the calendar event)
        // and only do a inorder traversal until the upper limit of the range is reached.
        let search: Vec<&&CalendarEvent> = self
            .tree
            .range(date..date.add(duration))
            .map(|f| f.1)
            .collect();

        return search;
    }

    pub fn get_by_uid(&self, uid: String) -> Option<&CalendarEvent> {
        // this is fast because any calendar event is also indexed inside the hashmap
        // two calendar event uid can be in the same bucket; however using a hashmap drastically
        // reduces the number of comparaisons compared to a linear search.
        self.uid_index.get(&uid)
    }

    pub fn update(&'a mut self, event: CalendarEvent) -> UpdateResult {
        if let Some(existing) = self.uid_index.get_mut(&event.uid) {
            if *existing == event {
                UpdateResult::Unchanged
            } else {
                self.tree.remove(&existing.start);
                let old = existing.clone();
                *existing = event;

                self.tree.insert(existing.start, &*existing);

                UpdateResult::Updated(old)
            }
        } else {
            UpdateResult::Created
        }
    }
}
