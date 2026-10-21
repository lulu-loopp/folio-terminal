//! A call on a first-party trait object: every implementation, and nothing else of the name; a
//! trait with no implementation and no default is a dynamic edge; a foreign trait object is one
//! too, with the first-party implementations as its candidates.

pub trait Driver {
    fn prepare(&self);
    fn settle(&self) {}
}

pub struct Mac;
pub struct Win;
pub struct Cache;

impl Driver for Mac {
    fn prepare(&self) {}
    fn settle(&self) {}
}

impl Driver for Win {
    fn prepare(&self) {}
}

impl Cache {
    pub fn prepare(&self) {}
}

pub fn borrowed(driver: &dyn Driver) {
    driver.prepare();
}

pub fn boxed(driver: Box<dyn Driver + Send>) {
    driver.prepare();
}

pub fn bound<D: Driver>(driver: &D) {
    driver.settle();
}

pub fn path(driver: &dyn Driver) {
    Driver::prepare(driver);
}

pub trait Lonely {
    fn alone(&self);
}

pub fn lonely(value: &dyn Lonely) {
    value.alone();
}

pub fn standard(sink: &mut dyn std::io::Write) {
    let _ = sink.flush();
}

pub struct Sink;

impl std::io::Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
