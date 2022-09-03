use std::{collections::HashMap, sync::Arc};

use bytes::Buf;
use chrono::NaiveDateTime;
use futures::Future;
use log::debug;

use crate::{cfg::{Config, ICalWatchItem}, calendar::store::CalendarEvent};

use self::store::Calendar;
pub mod store;

pub struct CalendarWatcher<'a> {
    config: Arc<Config>,
    hashmap: HashMap<String, Calendar<'a>>,
}

impl CalendarWatcher<'_> {
    pub fn new(config: Arc<Config>) -> Self {
        CalendarWatcher {
            config,
            hashmap: HashMap::new(),
        }
    }

    #[inline]
    async fn fetch_task(watch_item: &ICalWatchItem) -> Result<Calendar, reqwest::Error> {
        let data = reqwest::get(&watch_item.source)
            .await?
            .bytes()
            .await?
            .reader();

        let parser = ical::IcalParser::new(data);
        let calendar = Calendar::new();

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
                                        NaiveDateTime::parse_from_str(&value, "%Y%m%dT%H%M%SZ")
                                            .unwrap();
                                }
                                "DTEND" => {
                                    debug!("Parsing DTEND: {}", value);
                                    cal_event.end =
                                        NaiveDateTime::parse_from_str(&value, "%Y%m%dT%H%M%SZ")
                                            .unwrap();
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
                                        NaiveDateTime::parse_from_str(&value, "%Y%m%dT%H%M%SZ")
                                            .unwrap();
                                }
                                "CREATED" => {
                                    debug!("Parsing CREATED: {}", value);
                                    cal_event.created =
                                        NaiveDateTime::parse_from_str(&value, "%Y%m%dT%H%M%SZ")
                                            .unwrap();
                                }
                                "UID" => {
                                    cal_event.uid = value.to_string();
                                }
                                &_ => {}
                            }
                        }
                    }

                    println!("{:#?}", cal_event);
                }
            } else {
                continue;
            }
        }

        Ok(calendar)
    }

    #[inline]
    fn tasks(
        &self,
    ) -> impl Iterator<Item = impl Future<Output = (&String, Result<Calendar, reqwest::Error>)>>
    {
        self.config
            .calendar
            .watchers
            .iter()
            .map(|(name, object)| async move { (name, CalendarWatcher::fetch_task(object).await) })
    }

    #[allow(unused)]
    pub async fn update_calendars(&self) {
        let data = futures_util::future::join_all(self.tasks()).await;

        for (calendar, result) in data {
            match result {
                Ok(cal) => {}
                Err(err) => {}
            }
        }
    }
}
