#![allow(unused_imports)]
#![allow(unused_variables)]

use ncurses::*;
mod pub_message;
mod redis_receiver;

pub use pub_message::PublishMessage;
pub use redis_receiver::do_redis;

extern crate chrono;
extern crate crossbeam;
extern crate log;
extern crate rand;
extern crate redis;

use chrono::{DateTime, Local};

use crossbeam_channel::{bounded, unbounded, Receiver, Sender};
use log::{info, warn,debug,trace,error};
use log::LevelFilter;
use log4rs::append::file::FileAppender;
use log4rs::config::{Appender, Config, Root};
use log4rs::encode::pattern::PatternEncoder;
use std::cmp::Ordering;
use std::thread;
use std::time::{Duration, SystemTime};

const QUIT: i32 = 0x71;
const SORT_TOPIC: i32 = 'k' as i32;
const SORT_VALUE: i32 = 'v' as i32;
const SORT_TIME: i32 = 't' as i32;
const SORT_COUNT: i32 = 'c' as i32;

enum OrderSort {
    Topic,
    Value,
    Time,
    Count,
}

struct Entry {
    pub topic: String,
    pub value: String,
    pub time: DateTime<Local>,
    pub count: i32,
}

impl Entry {
    fn new(topic: String, value: String, time: DateTime<Local>) -> Entry {
        Entry {
            topic,
            value,
            time,
            count: 1,
        }
    }
    fn update(&mut self, pub_message: &PublishMessage) {
        self.value = pub_message.value.clone();
        self.time = pub_message.time;
        self.count += 1;
    }
}

struct EntryList {
    pub entries: Vec<Entry>,
}

impl EntryList {
    fn new() -> EntryList {
        EntryList {
            entries: Vec::new(),
        }
    }
    fn add(&mut self, pub_message: &PublishMessage) {
        let mut found = false;
        for entry in self.entries.iter_mut() {
            if entry.topic == pub_message.topic {
                entry.update(pub_message);
                found = true;
                break;
            }
        }
        if !found {
            self.entries.push(Entry::new(
                pub_message.topic.clone(),
                pub_message.value.clone(),
                pub_message.time,
            ));
        }
    }
}

fn display_list_ncurses(entries: &EntryList, layout: &Layout) {
    let mut row = 2;
    for entry in entries.entries.iter() {
        mvprintw(row, layout.topic_start + 1, &entry.topic);
        mvprintw(row, layout.value_start, &entry.value);
        mvprintw(
            row,
            layout.time_start,
            &format!("{}", entry.time.time().format("%H:%M:%S%.3f").to_string()),
        );
        mvprintw(row, layout.count_start, &format!("{}", entry.count));
        row += 1;
    }
}

#[derive(Copy, Clone)]

struct Size {
    width: i32,
    height: i32,
}
impl Size {
    fn new(width: i32, height: i32) -> Size {
        Size { width, height }
    }
    fn changed(&self, &rhs: &Size) -> bool {
        self.width != rhs.width || self.height != rhs.height
    }
    fn get_current_size() -> Size {
        let mut max_x = 0;
        let mut max_y = 0;
        getmaxyx(stdscr(), &mut max_y, &mut max_x);
        Size::new(max_x, max_y)
    }
}

struct Layout {
    size: Size,
    topic_start: i32,
    topic_width: i32,
    value_start: i32,
    value_width: i32,
    time_start: i32,
    time_width: i32,
    count_start: i32,
    count_width: i32,
    rows: i32,
}

// +-key-+-value-+-time-+-count-+
// |  20   |  50    |   20   |    10   |
// width -2

fn scale_percent(percent: i32, max: i32) -> i32 {
    (percent * max) / 100
}

impl Layout {
    fn new(size: Size) -> Layout {
        let hvec = vec![40, 40, 12, 8];
        let max = size.width - 2;
        let topic_start = 1;
        let topic_width = scale_percent(hvec[0], max);
        let value_start = topic_start + topic_width;
        let value_width = scale_percent(hvec[1], max);
        let time_start = value_start + value_width;
        let time_width = scale_percent(hvec[2], max);
        let count_start = time_start + time_width;
        let count_width = scale_percent(hvec[3], max);
        let rows = size.height - 1;
        Layout {
            size,
            topic_start,
            topic_width,
            value_start,
            value_width,
            time_start,
            time_width,
            count_start,
            count_width,
            rows,
        }
    }
    fn draw_header(&self) {
        attron(A_BOLD());
        mvprintw(1, self.topic_start, "Topic");
        mvprintw(1, self.value_start, "Value");
        mvprintw(1, self.time_start, "Time");
        mvprintw(1, self.count_start, "Count");
        attroff(A_BOLD());
    }
    fn draw_rectangle(&self) {
        let x = 0;
        let y = 0;
        for i in 0..self.size.width - 1 {
            mvaddch(y, x + i, ACS_HLINE());
            mvaddch(y + self.size.height - 1, x + i, ACS_HLINE());
        }
        for i in 0..self.size.height - 1 {
            mvaddch(y + i, x, ACS_VLINE());
            mvaddch(y + i, x + self.size.width - 1, ACS_VLINE());
        }
        mvaddch(y, x, ACS_ULCORNER());
        mvaddch(y, x + self.size.width - 1, ACS_URCORNER());
        mvaddch(y + self.size.height - 1, x, ACS_LLCORNER());
        mvaddch(
            y + self.size.height - 1,
            x + self.size.width - 1,
            ACS_LRCORNER(),
        );
    }
}

fn main() {
    let logfile = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new("{d} {l} {t} - {m}\n")))
        .build("log/output.log")
        .unwrap();

    let config = Config::builder()
        .appender(Appender::builder().build("logfile", Box::new(logfile)))
        .build(Root::builder().appender("logfile").build(LevelFilter::Info))
        .unwrap();

    log4rs::init_config(config).unwrap();

    let mut entry_list = EntryList::new();

    let (send, recv) = bounded::<PublishMessage>(200);

    thread::spawn(move || {
        info!("Starting Redis ");
        do_redis(send).unwrap();
    });

    let mut ordering = OrderSort::Topic;
    init_screen();
    timeout(1);
    let mut old_size = Size::new(0, 0);
    let mut new_size = Size::new(80, 24);
    let mut layout = Layout::new(new_size);
    loop {
        if Size::get_current_size().changed(&old_size) {
            old_size = new_size;
            new_size = Size::get_current_size();
            layout = Layout::new(new_size);
            clear();
            layout.draw_rectangle();
            layout.draw_header();
        }

        refresh();

        loop {
            let pub_message = recv.recv_timeout(Duration::from_millis(100));
            if pub_message.is_ok() {
                clear();
                layout.draw_rectangle();
                layout.draw_header();
                debug!("Received message: {:?}", pub_message);
                entry_list.add(&pub_message.unwrap());
                display_list_ncurses(&entry_list, &layout);
            } else {
                thread::sleep(Duration::from_millis(100));
                break;
            }
        }

        match ordering {
            OrderSort::Topic => {
                entry_list.entries.sort_by(|a, b| a.topic.cmp(&b.topic));
            }
            OrderSort::Value => {
                entry_list.entries.sort_by(|a, b| a.value.cmp(&b.value));
            }
            OrderSort::Time => {
                entry_list.entries.sort_by(|a, b| a.time.cmp(&b.time));
            }
            OrderSort::Count => {
                entry_list.entries.sort_by(|a, b| a.count.cmp(&b.count));
            }
        }

        let key = getch();
        match key {
            KEY_RESIZE => {
                mvprintw(20, 20, "Window was resized");
            }
            QUIT => {
                break;
            }
            SORT_COUNT => {
                ordering = OrderSort::Count;
            }
            SORT_TIME => {
                ordering = OrderSort::Time;
            }
            SORT_TOPIC => {
                ordering = OrderSort::Topic;
            }
            SORT_VALUE => {
                ordering = OrderSort::Value;
            }

            -1 => {} // No input
            _ => {
                let s = std::format!("The pressed key is {}", key);
                mvprintw(20, 20, s.as_str());
            }
        };
    }
    end_screen();
}

fn init_screen() {
    initscr();
    keypad(stdscr(), true);
    noecho();
}

fn end_screen() {
    endwin();
}
