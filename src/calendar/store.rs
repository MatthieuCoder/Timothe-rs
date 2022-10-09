use std::{
    collections::{BTreeMap, HashMap},
    fs, io,
    ops::Add,
    sync::Arc,
};

use anyhow::bail;
use chrono::{Duration, NaiveDateTime};
use serde::{Deserialize, Serialize};

use crate::cfg::Config;

pub enum UpdateResult {
    Created,
    Updated(Arc<CalendarEvent>),
    Unchanged,
}

#[derive(Debug, Default, Eq, PartialEq, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Calendar {
    // used to easily compute using dates
    tree: BTreeMap<NaiveDateTime, Arc<CalendarEvent>>,
    // used to search based on uids
    uid_index: HashMap<String, Arc<CalendarEvent>>,
}

impl<'a> Calendar {
    pub fn get_range(&self, date: NaiveDateTime, duration: Duration) -> Vec<Arc<CalendarEvent>> {
        // get all the events using the tree map
        // this is fast because we just search the binary tree (=few comparaisons to get to the leaf node containing the pointer to the calendar event)
        // and only do a inorder traversal until the upper limit of the range is reached.
        let search = self
            .tree
            .range(date..date.add(duration))
            .map(|f| f.1.clone())
            .collect();

        return search;
    }

    pub fn update(&'a mut self, event: CalendarEvent) -> UpdateResult {
        let event = Arc::new(event);

        if self.uid_index.contains_key(&event.uid) {
            let existing = self.uid_index.get_mut(&event.uid).expect("internal error");
            if *existing == event {
                UpdateResult::Unchanged
            } else {
                self.tree.remove(&existing.start);
                let old = existing.clone();
                *existing = event;

                self.tree.insert(existing.start, existing.clone());

                UpdateResult::Updated(old)
            }
        } else {
            let uid = event.uid.clone();
            self.uid_index.insert(uid.clone(), event);
            let moved_event = self.uid_index.get(&uid).expect("internal error");

            self.tree.insert(moved_event.start, moved_event.clone());
            UpdateResult::Created
        }
    }
}

pub type Data = HashMap<String, Calendar>;

#[derive(Debug)]
pub struct Store {
    pub data: Data,
    save_path: String,
}

impl Store {
    pub fn new<'b>(config: Arc<Config>) -> Result<Store, anyhow::Error> {
        let path = shellexpand::full_with_context_no_errors(
            &config.storage.path,
            || dirs::home_dir(),
            |f| std::env::var(f).ok(),
        )
        .to_string();

        match fs::read(&path) {
            Ok(r) => Ok(Store {
                data: postcard::from_bytes(&r)?,
                save_path: path,
            }),
            Err(err) => match err.kind() {
                // The only case where we can accept an error is when the db does not exists
                io::ErrorKind::NotFound => Ok(Store {
                    data: Data::default(),
                    save_path: path,
                }),
                _ => bail!(err),
            },
        }
    }

    pub fn apply(&mut self, calendar: String, events: Vec<CalendarEvent>) -> Result<Vec<UpdateResult>, anyhow::Error> {
        let cal = if let Some(calendar) = self.data.get_mut(&calendar) {
            calendar
        } else {
            self.data.insert(calendar.clone(), Calendar::default());

            self.data.get_mut(&calendar).expect("internal error")
        };

        // Returned updates values
        let value = events.into_iter().map(|elem| cal.update(elem)).collect();

        // Persist the db
        let data = postcard::to_allocvec(&self.data)?;
        fs::write(&self.save_path, data)?;

        Ok(value)
    }
}
