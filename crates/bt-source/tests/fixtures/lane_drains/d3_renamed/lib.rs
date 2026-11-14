//! The lane-drains guard's fixture (`tests/lane_drains.rs`): a lane seam in miniature, its two
//! drains, its door and the contract adapters. This is the green variant with one
//! defect planted, named by its directory: d3_renamed.

pub mod lane {
    pub struct Contract {
        pub delivery: Delivery,
    }

    pub enum Delivery {
        Submission,
        NewestAdopted,
    }

    pub const ASKS: Contract = Contract {
        delivery: Delivery::Submission,
    };

    pub const LATEST: Contract = Contract {
        delivery: Delivery::NewestAdopted,
    };

    pub struct Mark {
        owed: Option<u64>,
    }

    impl Mark {
        pub fn below(&self, id: u64) -> bool {
            self.owed.is_none_or(|owed| id < owed)
        }
    }

    pub struct Ledger {
        pub owed: Option<u64>,
    }

    impl Ledger {
        pub fn drain_mark(&self) -> Mark {
            Mark { owed: self.owed }
        }

        pub fn mark_after_the_worker_ended(&self) -> Mark {
            Mark { owed: None }
        }
    }

    pub struct Request {
        pub id: u64,
    }

    impl Request {
        pub fn take(&mut self, mark: &Mark) -> Option<u64> {
            mark.below(self.id).then_some(self.id)
        }
    }

    pub struct Coalescing {
        pub ledger: Ledger,
        pub held: Vec<Request>,
    }

    impl Coalescing {
        pub fn ended(&mut self) -> Vec<u64> {
            let mark = self.ledger.drain_mark();
            self.held
                .iter_mut()
                .filter_map(|request| request.take(&mark))
                .collect()
        }
    }

    pub fn in_admission_order(held: Vec<Vec<u64>>) -> Vec<u64> {
        let mut all: Vec<u64> = held.into_iter().flatten().collect();
        all.sort_unstable();
        all
    }
}

pub mod asks {
    use crate::lane::{self, Ledger, Request};

    pub struct Lane {
        pub ledger: Ledger,
    }

    impl Lane {
        pub fn drain_mark(&self) -> lane::Mark {
            self.ledger.drain_mark()
        }

        pub fn mark_after_the_worker_ended(&self) -> lane::Mark {
            self.ledger.mark_after_the_worker_ended()
        }
    }

    pub struct Window {
        pub requests: Vec<Request>,
    }

    impl Window {
        pub fn answered(&mut self, mark: &lane::Mark) -> Vec<u64> {
            self.requests
                .iter_mut()
                .filter_map(|request| request.take(mark))
                .collect()
        }
    }

    pub struct App {
        pub lane: Lane,
        pub windows: Vec<Window>,
    }

    impl App {
        pub fn answers_now(&mut self) -> Vec<u64> {
            let mark = self.lane.drain_mark();
            let mut held = Vec::new();
            for window in &mut self.windows {
                held.push(window.answered(&mark));
            }
            lane::in_admission_order(held)
        }

        pub fn at_the_stop(&mut self) -> Vec<u64> {
            let mark = self.lane.mark_after_the_worker_ended();
            self.windows
                .iter_mut()
                .flat_map(|window| window.answered(&mark))
                .collect()
        }
    }

    #[cfg(test)]
    mod contract_adapter {
        pub struct Adapter {
            pub app: super::App,
        }

        impl Adapter {
            pub fn drain(&mut self) -> Vec<u64> {
                self.app.answers_now()
            }
        }
    }
}

#[cfg(test)]
mod latest_adapter {
    pub struct Adapter {
        pub lane: crate::lane::Coalescing,
    }

    /// A second `ended`: the generic `held` leaves the adapter's call of it `Ambiguous`, and the
    /// rules' reviewed edge follows it.
    pub struct Other;

    impl Other {
        pub fn ended(&mut self) -> Vec<u64> {
            Vec::new()
        }
    }

    pub fn held<T>(value: &mut T) -> &mut T {
        value
    }

    impl Adapter {
        pub fn drain(&mut self) -> Vec<u64> {
            held(&mut self.lane).ended()
        }
    }
}
