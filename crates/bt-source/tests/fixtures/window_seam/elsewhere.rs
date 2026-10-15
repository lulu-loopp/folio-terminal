pub(crate) use crate::window_seam::WindowHost as Host;

pub struct Stray;

// PLANTED: a door of the seam implemented outside it.
impl Host for Stray {
    fn request_redraw(&self) {}
}

// PLANTED: a chain of renamed re-exports longer than the resolver follows, and an impl through its
// far end.
pub(crate) use crate::window_seam::WindowHost as Hop0;
pub(crate) use Hop0 as Hop1;
pub(crate) use Hop1 as Hop2;
pub(crate) use Hop2 as Hop3;
pub(crate) use Hop3 as Hop4;
pub(crate) use Hop4 as Hop5;
pub(crate) use Hop5 as Hop6;
pub(crate) use Hop6 as Hop7;
pub(crate) use Hop7 as Hop8;
pub(crate) use Hop8 as Hop9;
pub(crate) use Hop9 as Hop10;
pub(crate) use Hop10 as Hop11;
pub(crate) use Hop11 as Hop12;
pub(crate) use Hop12 as Hop13;
pub(crate) use Hop13 as Hop14;
pub(crate) use Hop14 as Hop15;
pub(crate) use Hop15 as Hop16;
pub(crate) use Hop16 as Hop17;
pub(crate) use Hop17 as Hop18;
pub(crate) use Hop18 as Hop19;
pub(crate) use Hop19 as Hop20;

pub struct Far;

impl Hop20 for Far {
    fn request_redraw(&self) {}
}

// PLANTED: a chain of renamed re-exports of the backend trait longer than the resolver follows.
pub(crate) use crate::window_seam::Backend as Bk0;
pub(crate) use Bk0 as Bk1;
pub(crate) use Bk1 as Bk2;
pub(crate) use Bk2 as Bk3;
pub(crate) use Bk3 as Bk4;
pub(crate) use Bk4 as Bk5;
pub(crate) use Bk5 as Bk6;
pub(crate) use Bk6 as Bk7;
pub(crate) use Bk7 as Bk8;
pub(crate) use Bk8 as Bk9;
pub(crate) use Bk9 as Bk10;
pub(crate) use Bk10 as Bk11;
pub(crate) use Bk11 as Bk12;
pub(crate) use Bk12 as Bk13;
pub(crate) use Bk13 as Bk14;
pub(crate) use Bk14 as Bk15;
pub(crate) use Bk15 as Bk16;
pub(crate) use Bk16 as Bk17;
pub(crate) use Bk17 as Bk18;
pub(crate) use Bk18 as Bk19;
pub(crate) use Bk19 as Bk20;
