extern crate redis;

use crate::PublishMessage;
use chrono::{DateTime, Local, Utc};
use crossbeam_channel::{bounded, unbounded, Receiver, Sender};
use log::info;
use redis::{Cmd, Commands, ConnectionLike, RedisError, RedisResult};
use std::thread;

pub fn do_redis(sender: Sender<PublishMessage>) -> redis::RedisResult<()> {
    info!("Starting redis receiver... ");
    loop {
        let client = redis::Client::open("redis://192.168.0.102/")?;
        let mut con = client.get_connection()?;
        let mut pubsub = con.as_pubsub();
        pubsub.psubscribe("*")?;
        info!("Subscribed to *");
        loop {
            let msg = pubsub.get_message();
            match msg {
                Ok(msg) => {
                    let payload: String = msg.get_payload()?;
                    let channel_name: String = msg.get_channel_name().to_string();
                    let pub_msg = PublishMessage {
                        topic: channel_name,
                        value: payload,
                        time: Local::now(),
                    };
                    sender.send(pub_msg.clone()).unwrap();
                }
                Err(e) => {
                    info!("Connection lost ? Error: {:?}", e);
                    break;
                }
            }
        }
    }
}
