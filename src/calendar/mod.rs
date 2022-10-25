use std::{collections::HashMap, sync::Arc};

use bytes::Buf;
use chrono::{NaiveDateTime, Utc};
use futures::Future;
use log::{debug, error};
use regex::Regex;

use crate::{
    calendar::store::CalendarEvent,
    cfg::{CalendarItem, Config},
};

use self::store::{Store, UpdateResult};
pub mod store;

pub struct CalendarWatcher {
    config: Arc<Config>,
    pub store: Store,
}

type CalendarParsed = Vec<CalendarEvent>;

impl CalendarWatcher {
    pub fn new(config: Arc<Config>) -> Result<Self, anyhow::Error> {
        Ok(CalendarWatcher {
            config: config.clone(),
            store: Store::new(config)?,
        })
    }

    #[inline]
    async fn fetch_task(watch_item: &CalendarItem) -> Result<CalendarParsed, anyhow::Error> {
        let data = reqwest::get(&watch_item.source)
            .await?
            .bytes()
            .await?
            .reader();

        let parser = ical::IcalParser::new(data);
        let mut events = CalendarParsed::new();

        for cal in parser {
            if let Ok(calendar) = cal {
                for event in calendar.events {
                    let mut cal_event: CalendarEvent = CalendarEvent::default();

                    for property in &event.properties {
                        if let Some(value) = &property.value {
                            match &property.name as &str {
                                "DTSTART" => {
                                    debug!("Parsing DTSTART: {}", value);
                                    cal_event.start =
                                        NaiveDateTime::parse_from_str(&value, "%Y%m%dT%H%M%SZ")?;
                                }
                                "DTEND" => {
                                    debug!("Parsing DTEND: {}", value);
                                    cal_event.end =
                                        NaiveDateTime::parse_from_str(&value, "%Y%m%dT%H%M%SZ")?;
                                }
                                "SUMMARY" => {
                                    cal_event.summary = value.to_string();
                                }
                                "LOCATION" => {
                                    cal_event.location = value.to_string();
                                }
                                "DESCRIPTION" => {
                                    let re = Regex::new(r"\(Exported\s.\d{2}\/\d{2}\/\d{4}\s\d{2}\:\d{2}\)").unwrap();

                                    cal_event.description = re.replace_all(value, "").to_string();
                                }
                                "LAST-MODIFIED" => {
                                    debug!("Parsing LAST-MODIFIED: {}", value);
                                    cal_event.last_modified =
                                        NaiveDateTime::parse_from_str(&value, "%Y%m%dT%H%M%SZ")?;
                                }
                                "CREATED" => {
                                    debug!("Parsing CREATED: {}", value);
                                    cal_event.created =
                                        NaiveDateTime::parse_from_str(&value, "%Y%m%dT%H%M%SZ")?;
                                }
                                "UID" => {
                                    cal_event.uid = value.to_string();
                                }
                                &_ => {}
                            }
                        }
                    }

                    events.push(cal_event);
                }
            } else {
                continue;
            }
        }

        Ok(events)
    }

    #[inline]
    fn tasks<'b>(
        config: &'b Config,
    ) -> impl Iterator<
        Item = impl Future<Output = (String, NaiveDateTime, Result<CalendarParsed, anyhow::Error>)> + 'b,
    > {
        config
            .calendar
            .watchers
            .iter()
            .map(|(name, object)| async move {
                let result = CalendarWatcher::fetch_task(object).await;
                (name.to_string(), Utc::now().naive_local(), result)
            })
    }

    #[allow(unused)]
    pub async fn update_calendars(&mut self) -> HashMap<std::string::String, Vec<UpdateResult>> {
        let data = {
            let tasks = CalendarWatcher::tasks(&self.config);
            let data = futures_util::future::join_all(tasks).await;

            data
        };
        let store = &mut self.store;

        let mut calendars = HashMap::new();

        for (calendar_name, fetch_date, result) in data {
            match result {
                Ok(cal) => {
                    calendars.insert(
                        calendar_name.clone(),
                        store
                            .apply(calendar_name.clone(), cal, fetch_date)
                            .expect("failed to update calendar"),
                    );
                }
                Err(err) => {
                    error!(
                        "failed to parse events for calendars {}: {}",
                        calendar_name, err
                    );
                }
            }
        }

        calendars
    }
}
