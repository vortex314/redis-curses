extern crate chrono;

use chrono::{DateTime, Local};

#[derive(Debug,Clone)]
pub struct PublishMessage {
    pub topic: String,
    pub value: String,
    pub time: DateTime<Local>,
}

