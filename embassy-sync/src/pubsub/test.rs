use super::*;
use crate::blocking_mutex::raw::NoopRawMutex;

// #[futures_test::test]
// async fn dyn_pub_sub_works() {
//     let channel = PubSubChannel::<NoopRawMutex, u32, 4, 4, 4>::new();

//     let mut sub0 = channel.dyn_subscriber().unwrap();
//     let mut sub1 = channel.dyn_subscriber().unwrap();
//     let pub0 = channel.dyn_publisher().unwrap();

//     pub0.publish(42).await;

//     assert_eq!(sub0.next_message().await, WaitResult::Message(42));
//     assert_eq!(sub1.next_message().await, WaitResult::Message(42));

//     assert_eq!(sub0.try_next_message(), None);
//     assert_eq!(sub1.try_next_message(), None);
// }

#[futures_test::test]
async fn all_subscribers_receive() {
    let channel = PubSubChannel::<NoopRawMutex, u32, 4>::new();

    let mut sub0 = channel.subscriber();
    let mut sub1 = channel.subscriber();
    let mut pub0 = channel.publisher();

    pub0.publish(42).await;

    assert_eq!(sub0.next_message().await, WaitResult::Message(42));
    assert_eq!(sub1.next_message().await, WaitResult::Message(42));

    assert_eq!(sub0.try_next_message(), None);
    assert_eq!(sub1.try_next_message(), None);
}

#[futures_test::test]
async fn lag_when_queue_full_on_immediate_publish() {
    let channel = PubSubChannel::<NoopRawMutex, u32, 4>::new();

    let mut sub0 = channel.subscriber();
    let pub0 = channel.publisher();

    pub0.publish_immediate(42);
    pub0.publish_immediate(43);
    pub0.publish_immediate(44);
    pub0.publish_immediate(45);
    pub0.publish_immediate(46);
    pub0.publish_immediate(47);

    assert_eq!(sub0.try_next_message(), Some(WaitResult::Lagged(2)));
    assert_eq!(sub0.next_message().await, WaitResult::Message(44));
    assert_eq!(sub0.next_message().await, WaitResult::Message(45));
    assert_eq!(sub0.next_message().await, WaitResult::Message(46));
    assert_eq!(sub0.next_message().await, WaitResult::Message(47));
    assert_eq!(sub0.try_next_message(), None);
}

#[test]
fn publisher_wait_on_full_queue() {
    let channel = PubSubChannel::<NoopRawMutex, u32, 4>::new();

    let pub0 = channel.publisher();

    // There are no subscribers, so the queue will never be full
    assert_eq!(pub0.try_publish(0), Ok(()));
    assert_eq!(pub0.try_publish(0), Ok(()));
    assert_eq!(pub0.try_publish(0), Ok(()));
    assert_eq!(pub0.try_publish(0), Ok(()));
    assert_eq!(pub0.try_publish(0), Ok(()));

    let sub0 = channel.subscriber();

    assert_eq!(pub0.try_publish(0), Ok(()));
    assert_eq!(pub0.try_publish(0), Ok(()));
    assert_eq!(pub0.try_publish(0), Ok(()));
    assert_eq!(pub0.try_publish(0), Ok(()));
    assert_eq!(pub0.try_publish(0), Err(0));

    drop(sub0);
}

#[futures_test::test]
async fn correct_available() {
    let channel = PubSubChannel::<NoopRawMutex, u32, 4>::new();

    let sub0 = channel.subscriber();
    let mut sub1 = channel.subscriber();
    let mut pub0 = channel.publisher();

    assert_eq!(sub0.available(), 0);
    assert_eq!(sub1.available(), 0);

    pub0.publish(42).await;

    assert_eq!(sub0.available(), 1);
    assert_eq!(sub1.available(), 1);

    sub1.next_message().await;

    assert_eq!(sub1.available(), 0);

    pub0.publish(42).await;

    assert_eq!(sub0.available(), 2);
    assert_eq!(sub1.available(), 1);
}

#[futures_test::test]
async fn correct_space() {
    let channel = PubSubChannel::<NoopRawMutex, u32, 4>::new();

    let mut sub0 = channel.subscriber();
    let mut sub1 = channel.subscriber();
    let mut pub0 = channel.publisher();

    assert_eq!(pub0.space(), 4);

    pub0.publish(42).await;

    assert_eq!(pub0.space(), 3);

    pub0.publish(42).await;

    assert_eq!(pub0.space(), 2);

    sub0.next_message().await;
    sub0.next_message().await;

    assert_eq!(pub0.space(), 2);

    sub1.next_message().await;
    assert_eq!(pub0.space(), 3);
    sub1.next_message().await;
    assert_eq!(pub0.space(), 4);
}

#[futures_test::test]
async fn empty_channel_when_last_subscriber_is_dropped() {
    let channel = PubSubChannel::<NoopRawMutex, u32, 4>::new();

    let mut pub0 = channel.publisher();
    let mut sub0 = channel.subscriber();
    let mut sub1 = channel.subscriber();

    assert_eq!(4, pub0.space());

    pub0.publish(1).await;
    pub0.publish(2).await;

    assert_eq!(2, channel.space());

    assert_eq!(1, sub0.try_next_message_pure().unwrap());
    assert_eq!(2, sub0.try_next_message_pure().unwrap());

    assert_eq!(2, channel.space());

    drop(sub0);

    assert_eq!(2, channel.space());

    assert_eq!(1, sub1.try_next_message_pure().unwrap());

    assert_eq!(3, channel.space());

    drop(sub1);

    assert_eq!(4, channel.space());
}

struct CloneCallCounter(usize);

impl Clone for CloneCallCounter {
    fn clone(&self) -> Self {
        Self(self.0 + 1)
    }
}

#[futures_test::test]
async fn skip_clone_for_last_message() {
    let channel = PubSubChannel::<NoopRawMutex, CloneCallCounter, 1>::new();
    let mut pub0 = channel.publisher();
    let mut sub0 = channel.subscriber();
    let mut sub1 = channel.subscriber();

    pub0.publish(CloneCallCounter(0)).await;

    assert_eq!(1, sub0.try_next_message_pure().unwrap().0);
    assert_eq!(0, sub1.try_next_message_pure().unwrap().0);
}
