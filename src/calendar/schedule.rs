use std::{
    collections::{BTreeMap, HashMap},
    fs, io,
    ops::Add,
    sync::Arc,
};

use anyhow::{bail, Context};
use chrono::{DateTime, Duration, Utc};
use log::{debug, info};
use serde::{Deserialize, Serialize};

use crate::cfg::{CalendarItem, Config};

use super::{Event, UpdateResult};

/// A calendar is a collection of events
/// and utility functions used to search and sort them.
#[derive(Debug)]
pub struct Calendar {
    // used to easily compute using dates
    tree: BTreeMap<DateTime<Utc>, Arc<Event>>,
    // used to search based on uids
    uid_index: HashMap<String, Arc<Event>>,
}

impl<'de> Deserialize<'de> for Calendar {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let elements: Vec<Arc<Event>> = Vec::deserialize(deserializer)?;
        let mut tree = BTreeMap::new();
        let mut uid_index = HashMap::new();

        for item in elements {
            tree.insert(item.start, item.clone());
            uid_index.insert(item.uid.clone(), item);
        }

        Ok(Self { tree, uid_index })
    }
}

impl Serialize for Calendar {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let elements: Vec<Arc<Event>> = self.uid_index.iter().map(|c| c.1.clone()).collect();

        elements.serialize(serializer)
    }
}

impl Calendar {
    pub fn get_range(&self, date: DateTime<Utc>, duration: Duration) -> Vec<Arc<Event>> {
        // get all the events using the tree map
        // this is fast because we just search the binary tree (=few comparaisons to get to the leaf node containing the pointer to the calendar event)
        // and only do a inorder traversal until the upper limit of the range is reached.
        let search = self
            .tree
            .range(date..date.add(duration))
            .map(|f| f.1.clone())
            .collect();

        search
    }

    pub fn new() -> Self {
        Self {
            tree: BTreeMap::new(),
            uid_index: HashMap::new(),
        }
    }

    /// Updates an event in a calendar
    /// Returns a list of edits made by the program to match the given calendar
    /// WIP: This algorithm needs heavy optimization and is used only for testing purposes
    pub fn update(
        &mut self,
        events: Vec<Event>,
        fetch_time: DateTime<Utc>,
        config: &CalendarItem,
    ) -> Result<Vec<UpdateResult>, anyhow::Error> {
        // use a tree of the indexed data for better handling
        let tree_index = BTreeMap::from_iter(events.into_iter().map(|f| {
            info!("Indexing event at {}", f.start);
            (f.start, Arc::new(f))
        }));
        info!("Updating calendar with {} events", tree_index.len());

        // compute the last event stored in the current calendar
        let existing_end = *self
            .tree
            .iter()
            .next()
            .map_or(&DateTime::<Utc>::MAX_UTC, |f| f.0);

        let mut updates = vec![];

        // for each event we want to add
        for new in tree_index.values() {
            info!("1a: Processing event at {}", new.start);
            // if the event already exists, we want to update the event and emit an event
            if self.uid_index.contains_key(&new.uid) {
                let existing = self
                    .uid_index
                    .get_mut(&new.uid)
                    .context("expected an event to be in the uid_index, but it wasn't present")?;

                // if the event is different, we want to update it
                if existing != new {
                    let old = existing.clone();
                    // update the uid index
                    *existing = new.clone();
                    // update in the tree
                    self.tree.insert(new.start, new.clone());
                    self.tree.remove(&old.start);

                    // emit the event
                    updates.push(UpdateResult::Updated {
                        old,
                        new: new.clone(),
                    });
                }
            } else {
                // we want to create the event

                info!("adding new event at {}", new.start);

                let uid = new.uid.clone();
                self.uid_index.insert(uid, new.clone());
                self.tree.insert(new.start, new.clone());

                // we should emit an update only if the event is added before the last event present at the start.
                if new.start < existing_end {
                    updates.push(UpdateResult::Created(new.clone()));
                } else {
                    debug!("not emitting a created event for {} because it's after the last event present at the start ({})", new.start, existing_end);
                }
            }
        }

        let end_slice = fetch_time
            + Duration::from_std(
                humantime::parse_duration(&config.time_amount)
                    .context("invalid format in the time_amount duration")?,
            )
            .context("failed to get a duration from standard")?;

        // we get all the events present in the range [add_start,add_end]
        // this is used to check if there are events that were deleted
        let range: Vec<Arc<Event>> = self
            .tree
            .range(fetch_time..end_slice)
            .map(|f| f.1.clone())
            .collect();

        info!(
            "Processing {} events [{} - {}]",
            range.len(),
            fetch_time,
            end_slice
        );

        // now we are going to check if there are deleted events in the stored range
        for event in range {
            if !tree_index.contains_key(&event.start) {
                // event need to be removed
                self.tree.remove(&event.start);
                let old = self
                    .uid_index
                    .remove(&event.uid)
                    .context("should happen. the key wasn't in the hashmap")?;

                updates.push(UpdateResult::Removed(old));
            }
        }

        Ok(updates)
    }
}

pub type Data = HashMap<String, Calendar>;

#[derive(Debug)]
pub struct Store {
    pub data: Data,
    config: Arc<Config>,
    save_path: String,
}

impl Store {
    pub fn new(config: Arc<Config>) -> Result<Self, anyhow::Error> {
        let path = shellexpand::full_with_context_no_errors(
            &config.storage.path,
            || dirs::home_dir().and_then(|p| p.to_str().map(|s| s.to_string())),
            |f| std::env::var(f).ok(),
        )
        .to_string();

        match fs::read(&path) {
            Ok(r) => Ok(Self {
                data: postcard::from_bytes(&r)?,
                config,
                save_path: path,
            }),
            Err(err) => match err.kind() {
                // The only case where we can accept an error is when the db does not exists
                io::ErrorKind::NotFound => Ok(Self {
                    data: Data::default(),
                    save_path: path,
                    config,
                }),
                _ => bail!(err),
            },
        }
    }

    pub fn apply(
        &mut self,
        calendar: &str,
        events: Vec<Event>,
        fetch_time: DateTime<Utc>,
    ) -> Result<Vec<UpdateResult>, anyhow::Error> {
        let cal = if let Some(calendar) = self.data.get_mut(calendar) {
            calendar
        } else {
            debug!("init: calendar: {}", calendar);
            self.data.insert(calendar.to_string(), Calendar::new());

            self.data
                .get_mut(calendar)
                .context("couldn't insert the calendar in the hashmap")?
        };
        let config = self
            .config
            .calendar
            .calendars
            .get(calendar)
            .context("unknown calendar: unreachable")?
            .clone();
        // Returned updates values
        let value = cal.update(events, fetch_time, &config)?;

        // Persist the db
        let data = postcard::to_allocvec(&self.data)?;
        fs::write(&self.save_path, data)?;

        Ok(value)
    }
}

// #[cfg(test)]
// mod test {
//     use std::sync::Arc;
//
//     use chrono::{DateTime, NaiveDateTime, Utc};
//     use poise::serenity_prelude::{ChannelId, RoleId};
//
//     use crate::cfg::CalendarItem;
//
//     use super::{Calendar, Event, UpdateResult};
//
//     #[test]
//     fn add_events() {
//         // use a calendar with two weeks checks
//         let mut cal: Calendar = Calendar::new();
//         let conf = CalendarItem {
//             source: String::default(),
//             channel: vec![ChannelId::new(0)],
//             role: vec![RoleId::new(0)],
//             time_amount: "2w".to_string(),
//         };
//
//         let test_events = vec![
//             Event {
//                 summary: "test event1".to_string(),
//                 start: DateTime::from_timestamp(0, 0).unwrap(),
//                 end: DateTime::from_timestamp(60, 0).unwrap(),
//                 location: "".to_string(),
//                 description: "".to_string(),
//                 uid: "000".to_string(),
//             },
//             Event {
//                 summary: "test event1".to_string(),
//                 start: DateTime::from_timestamp(60, 0).unwrap(),
//                 end: DateTime::from_timestamp(120, 0).unwrap(),
//                 location: "".to_string(),
//                 description: "".to_string(),
//                 uid: "002".to_string(),
//             },
//         ];
//
//         let updates = cal
//             .update(
//                 test_events.clone(),
//                 DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
//                 &conf,
//             )
//             .unwrap();
//
//         let expected = vec![
//             UpdateResult::Created(Arc::new(test_events[0].clone())),
//             UpdateResult::Created(Arc::new(test_events[1].clone())),
//         ];
//
//         assert_eq!(updates, expected);
//     }
//
//     #[test]
//     fn edit_events() {
//         let mut cal: Calendar = Calendar::new();
//
//         let conf = CalendarItem {
//             source: String::default(),
//             channel: vec![ChannelId::new(0)],
//             role: vec![RoleId::new(0)],
//             time_amount: "2w".to_string(),
//         };
//
//         let test_events = vec![
//             Event {
//                 summary: "test event1".to_string(),
//                 start: DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(0, 0), Utc),
//                 end: DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(60, 0), Utc),
//                 location: "".to_string(),
//                 description: "".to_string(),
//                 uid: "000".to_string(),
//             },
//             Event {
//                 summary: "test event1".to_string(),
//                 start: DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(60, 0), Utc),
//                 end: DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(120, 0), Utc),
//                 location: "".to_string(),
//                 description: "".to_string(),
//                 uid: "002".to_string(),
//             },
//         ];
//
//         let inserts = cal
//             .update(
//                 test_events.clone(),
//                 NaiveDateTime::from_timestamp(0, 0),
//                 &conf,
//             )
//             .unwrap();
//
//         let expected = vec![
//             UpdateResult::Created(Arc::new(test_events[0].clone())),
//             UpdateResult::Created(Arc::new(test_events[1].clone())),
//         ];
//
//         assert_eq!(inserts, expected);
//
//         let updates_data = vec![
//             Event {
//                 summary: "test event1".to_string(),
//                 start: NaiveDateTime::from_timestamp_opt(0, 0).unwrap(),
//                 end: NaiveDateTime::from_timestamp_opt(60, 0).unwrap(),
//                 location: "".to_string(),
//                 description: "this is updated".to_string(),
//                 uid: "000".to_string(),
//             },
//             Event {
//                 summary: "test event1".to_string(),
//                 start: NaiveDateTime::from_timestamp_opt(65, 0).unwrap(),
//                 end: NaiveDateTime::from_timestamp_opt(120, 0).unwrap(),
//                 location: "".to_string(),
//                 description: "this is updated".to_string(),
//                 uid: "002".to_string(),
//             },
//         ];
//
//         let updates = cal
//             .update(
//                 updates_data.clone(),
//                 NaiveDateTime::from_timestamp(0, 0),
//                 &conf,
//             )
//             .unwrap();
//
//         let expected = vec![
//             UpdateResult::Updated {
//                 old: Arc::new(test_events[0].clone()),
//                 new: Arc::new(updates_data[0].clone()),
//             },
//             UpdateResult::Updated {
//                 old: Arc::new(test_events[1].clone()),
//                 new: Arc::new(updates_data[1].clone()),
//             },
//         ];
//
//         assert_eq!(updates, expected);
//     }
//
//     #[test]
//     fn remove_test() {
//         let mut cal: Calendar = Calendar::new();
//
//         let conf = CalendarItem {
//             source: String::default(),
//             channel: vec![ChannelId::new(0)],
//             role: vec![RoleId::new(0)],
//             time_amount: "2w".to_string(),
//         };
//
//         let test_events = vec![
//             Event {
//                 summary: "test event1".to_string(),
//                 start: NaiveDateTime::from_timestamp(0, 0),
//                 end: NaiveDateTime::from_timestamp(60, 0),
//                 location: "".to_string(),
//                 description: "".to_string(),
//                 uid: "000".to_string(),
//             },
//             Event {
//                 summary: "test event2".to_string(),
//                 start: NaiveDateTime::from_timestamp(60, 0),
//                 end: NaiveDateTime::from_timestamp(120, 0),
//                 location: "".to_string(),
//                 description: "".to_string(),
//                 uid: "002".to_string(),
//             },
//             Event {
//                 summary: "test event3".to_string(),
//                 start: NaiveDateTime::from_timestamp(120, 0),
//                 end: NaiveDateTime::from_timestamp(180, 0),
//                 location: "".to_string(),
//                 description: "".to_string(),
//                 uid: "003".to_string(),
//             },
//         ];
//
//         cal.update(
//             test_events.clone(),
//             NaiveDateTime::from_timestamp_opt(0, 0).unwrap(),
//             &conf,
//         )
//         .unwrap();
//
//         let updates_data = vec![];
//
//         let updates = cal
//             .update(
//                 updates_data,
//                 NaiveDateTime::from_timestamp_opt(0, 0).unwrap(),
//                 &conf,
//             )
//             .unwrap();
//
//         let expected = vec![
//             UpdateResult::Removed(Arc::new(test_events[0].clone())),
//             UpdateResult::Removed(Arc::new(test_events[1].clone())),
//             UpdateResult::Removed(Arc::new(test_events[2].clone())),
//         ];
//
//         assert_eq!(updates, expected);
//     }
//
//     #[test]
//     fn remove_test_2() {
//         let mut cal: Calendar = Calendar::new();
//
//         let conf = CalendarItem {
//             source: String::default(),
//             channel: vec![ChannelId::new(0)],
//             role: vec![RoleId::new(0)],
//             time_amount: "2w".to_string(),
//         };
//         let test_events = vec![
//             Event {
//                 summary: "test event1".to_string(),
//                 start: DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(0, 0), Utc),
//                 end: DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(60, 0), Utc),
//                 location: "".to_string(),
//                 description: "".to_string(),
//                 uid: "000".to_string(),
//             },
//             Event {
//                 summary: "test event2".to_string(),
//                 start: DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(60, 0), Utc),
//                 end: DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(120, 0), Utc),
//                 location: "".to_string(),
//                 description: "".to_string(),
//                 uid: "002".to_string(),
//             },
//             Event {
//                 summary: "test event3".to_string(),
//                 start: DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(120, 0), Utc),
//                 end: DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(180, 0), Utc),
//                 location: "".to_string(),
//                 description: "".to_string(),
//                 uid: "003".to_string(),
//             },
//         ];
//
//         cal.update(
//             test_events.clone(),
//             DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(0, 0), Utc),
//             &conf,
//         )
//         .unwrap();
//
//         let updates_data = vec![
//             Event {
//                 summary: "test event1".to_string(),
//                 start: DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(0, 0), Utc),
//                 end: DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(60, 0), Utc),
//                 location: "".to_string(),
//                 description: "".to_string(),
//                 uid: "000".to_string(),
//             },
//             Event {
//                 summary: "test event3".to_string(),
//                 start: DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(120, 0), Utc),
//                 end: DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(180, 0), Utc),
//                 location: "".to_string(),
//                 description: "".to_string(),
//                 uid: "003".to_string(),
//             },
//         ];
//
//         let updates = cal
//             .update(
//                 updates_data,
//                 DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(0, 0), Utc),
//                 &conf,
//             )
//             .unwrap();
//
//         let expected = vec![UpdateResult::Removed(Arc::new(test_events[1].clone()))];
//
//         assert_eq!(updates, expected);
//     }
// }
//
