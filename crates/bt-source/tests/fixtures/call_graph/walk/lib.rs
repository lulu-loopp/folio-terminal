//! The walk: a cycle visited once, a boundary not entered, the first path kept.

pub fn root() {
    ping();
    boundary();
}

fn ping() {
    pong();
}

fn pong() {
    ping();
    let _ = std::fs::read("x");
}

fn boundary() {
    beyond();
}

fn beyond() {}
