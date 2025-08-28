use std::{collections::HashMap, sync::Arc};

use anyhow::Context;
use bytes::Buf;
use chrono::{DateTime, NaiveDateTime, Utc};
use futures::Future;
use log::{debug, error, info};
use regex::Regex;

use crate::cfg::{CalendarItem, Config};

use super::{schedule::Store, Event, UpdateResult};

pub struct Manager {
    config: Arc<Config>,
    pub store: Store,
}

impl Manager {
    pub fn new(config: Arc<Config>) -> Result<Self, anyhow::Error> {
        Ok(Self {
            config: config.clone(),
            store: Store::new(config)?,
        })
    }

    #[inline]
    async fn fetch_task(watch_item: &CalendarItem) -> Result<Vec<Event>, anyhow::Error> {
        let response = reqwest::get(&watch_item.source).await?.error_for_status()?;

        let data = response.bytes().await?.reader();

        let parser = ical::IcalParser::new(data);
        let mut events = Vec::new();

        for calendar in parser.flatten() {
            for event in calendar.events {
                let mut cal_event: Event = Event::default();

                for property in &event.properties {
                    if let Some(value) = &property.value {
                        match &property.name as &str {
                            "DTSTART" => {
                                debug!("Parsing DTSTART: {}", value);
                                let ndt = NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%SZ")?;
                                cal_event.start = ndt.and_utc();
                            }
                            "DTEND" => {
                                debug!("Parsing DTEND: {}", value);
                                let ndt = NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%SZ")?;
                                cal_event.end = ndt.and_utc();
                            }
                            "SUMMARY" => {
                                cal_event.summary = value.trim().to_string();
                            }
                            "LOCATION" => {
                                cal_event.location = value.to_string();
                            }
                            "DESCRIPTION" => {
                                let re = Regex::new(r"\(.*\)")
                                    .context("failed to build regex expression")?;

                                cal_event.description =
                                    re.replace_all(value, "").trim().to_string();
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
        }

        info!("Fetched {} events from {}", events.len(), watch_item.source);

        Ok(events)
    }

    #[inline]
    fn tasks(
        config: &Config,
    ) -> impl Iterator<
        Item = impl Future<Output = (String, DateTime<Utc>, Result<Vec<Event>, anyhow::Error>)> + '_,
    > {
        config
            .calendar
            .calendars
            .iter()
            .map(|(name, object)| async move {
                let result = Self::fetch_task(object).await;
                (name.to_string(), Utc::now(), result)
            })
    }

    #[allow(unused)]
    pub async fn update_calendars(
        &mut self,
    ) -> Result<HashMap<std::string::String, Vec<UpdateResult>>, anyhow::Error> {
        let data = {
            let tasks = Self::tasks(&self.config);
            let data = futures_util::future::join_all(tasks).await;

            data
        };
        let store = &mut self.store;

        let mut calendars = HashMap::new();

        for (calendar_name, fetch_date, result) in data {
            match result {
                Ok(cal) => {
                    info!("updating calendar {} with {} events", calendar_name, cal.len());
                    calendars.insert(
                        calendar_name.clone(),
                        store
                            .apply(&calendar_name, cal, fetch_date)
                            .context("failed to update calendar")?,
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

        Ok(calendars)
    }
}
