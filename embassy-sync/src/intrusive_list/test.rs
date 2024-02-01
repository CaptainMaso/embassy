use core::pin::pin;
use core::sync::atomic::{AtomicBool, AtomicUsize};

use rand::prelude::*;
use rand::SeedableRng;

use super::*;

struct Loud(usize);

impl Loud {
    pub fn new(v: usize) -> Self {
        eprintln!("Init({v})");

        Loud(v)
    }

    pub fn ping(&mut self, local: bool, thread_id: usize) {
        eprintln!(
            "Ping({} from {thread_id} for {})",
            if local { "local" } else { "remote" },
            self.0
        );
    }
}

impl Drop for Loud {
    fn drop(&mut self) {
        eprintln!("Dropped({})", self.0);
    }
}

type List = super::IntrusiveList<Loud, std::sync::Mutex<()>>;

const fn new_list() -> List {
    List::new_with(std::sync::Mutex::new(()))
}

#[derive(Debug, Clone, Copy)]
enum Ops {
    Register,
    Ping,
    PingAll,
    Deregister,
    Close,
}

impl Ops {
    pub fn gen(rand: &mut impl rand::Rng) -> Self {
        let v: u8 = rand.gen_range(0..4);
        match v {
            0 => Self::Register,
            1 => Self::Ping,
            2 => Self::Deregister,
            3 => Self::Close,
            _ => unreachable!(),
        }
    }
}

#[test]
fn insert_tail() {
    let list = new_list();

    let v1 = pin!(list.new_store(Loud::new(1)));
    let v2 = pin!(list.new_store(Loud::new(2)));
    let v3 = pin!(list.new_store(Loud::new(3)));
    let v4 = pin!(list.new_store(Loud::new(4)));

    list.with_cursor(|s| {
        s.insert_tail(v1.as_ref());
        s.insert_tail(v2.as_ref());
        s.insert_tail(v3.as_ref());
        s.insert_tail(v4.as_ref());

        s.seek(0);

        s.for_each(|idx, v| {
            v.ping(true, 0);
            assert!(idx == v.0);
        });

        s.seek(0);
        s.remove();

        s.for_each(|idx, v| {
            v.ping(true, 0);
            assert!(idx + 1 == v.0);
        });
    })
}

//#[test]
fn exercise() {
    let list = new_list();
    let finished = AtomicBool::new(false);
    let thread_id = AtomicUsize::new(0);
    let start = std::time::Instant::now();

    std::thread::scope(|s| {
        let mut rand = rand::rngs::StdRng::seed_from_u64(12345678);
        while thread_id.load(core::sync::atomic::Ordering::SeqCst) < 32 {
            if rand.gen::<u16>() < 100 {
                s.spawn(|| {
                    let tid = thread_id.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
                    eprintln!("Starting thread {tid}");
                    let mut store = list.new_store(Loud::new(tid));
                    let mut store = pin!(store);

                    let mut rand = rand::rngs::StdRng::seed_from_u64(tid as u64);
                    while !finished.load(core::sync::atomic::Ordering::Relaxed) {
                        match Ops::gen(&mut rand) {
                            Ops::Register => list.with_cursor(|c| {
                                c.insert_tail(store.as_ref());
                            }),
                            Ops::Ping => store.as_mut().lock(|p| p.ping(true, tid)),
                            Ops::PingAll => list.with_cursor(|l| l.for_each(|_, p| p.ping(false, tid))),
                            Ops::Deregister => store.as_ref().remove(),
                            Ops::Close => break,
                        }

                        let sleep = rand.gen_range(500..5_000);
                        std::thread::sleep(std::time::Duration::from_micros(sleep));
                    }
                });
            }

            std::thread::sleep(std::time::Duration::from_micros(500));
        }
        finished.store(true, core::sync::atomic::Ordering::Relaxed);
    });
}
