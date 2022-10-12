use std::sync::Arc;

use bytes::Buf;
use chrono::NaiveDateTime;
use futures::Future;
use log::{debug, error};

use crate::{
    calendar::store::CalendarEvent,
    cfg::{Config, ICalWatchItem},
};

use self::store::Store;
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
    async fn fetch_task(watch_item: &ICalWatchItem) -> Result<CalendarParsed, anyhow::Error> {
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
                                    cal_event.description = value.to_string();
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
    ) -> impl Iterator<Item = impl Future<Output = (String, Result<CalendarParsed, anyhow::Error>)> + 'b>
    {
        config
            .calendar
            .watchers
            .iter()
            .map(|(name, object)| async move {
                (name.to_string(), CalendarWatcher::fetch_task(object).await)
            })
    }

    #[allow(unused)]
    pub async fn update_calendars(&mut self) {
        let data = {
            let tasks = CalendarWatcher::tasks(&self.config);
            let data = futures_util::future::join_all(tasks).await;

            data
        };
        let store = &mut self.store;

        for (calendar_name, result) in data {
            match result {
                Ok(cal) => {
                    store.apply(calendar_name, cal);
                }
                Err(err) => {
                    error!("failed to parse events for calendars {}: {}", calendar_name, err);
                }
            }
        }
    }
}
