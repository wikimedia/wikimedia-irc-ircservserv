//! Interact with ChanServ
use crate::LockedState;
use irc::client::Client;
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;
use tokio::time::{interval, Duration};

/// Messages that go over the ChanServ channel
#[derive(Clone, Debug)]
pub enum Message {
    /// /cs flags <#channel>
    Flags(String),
    /// A NOTICE from ChanServ
    Notice(String),
}

/// Listen to messages on the ChanServ channel
pub async fn listen(
    rx: &mut Receiver<Message>,
    state: LockedState,
    client: Arc<Client>,
) {
    while let Some(notice) = rx.recv().await {
        match notice {
            Message::Flags(channel) => {
                if state.read().await.chanserv.is_some() {
                    // Someone else is reading from chanserv, please wait
                    let mut interval = interval(Duration::from_millis(200));
                    loop {
                        if state.read().await.chanserv.is_none() {
                            break;
                        }
                        interval.tick().await;
                    }
                }
                {
                    let mut w = state.write().await;
                    w.chanserv = Some(Message::Flags(channel.to_string()));
                }
                client
                    .send_privmsg("ChanServ", format!("flags {}", &channel))
                    .unwrap();
                continue;
            }
            Message::Notice(notice) => {
                // Clone instead of locking since we need to get the
                // write lock inside to clear it
                let looking = state.read().await.chanserv.clone();
                if notice.starts_with("--------------")
                    || notice.starts_with("Entry    Nickname/Host")
                {
                    continue;
                }
                if let Some(Message::Flags(channel)) = &looking {
                    if notice.starts_with("End of") {
                        let mut w = state.write().await;
                        w.channels.get_mut(channel).unwrap().flags_done = true;
                        w.chanserv = None;
                    } else {
                        let mut w = state.write().await;
                        let managed =
                            w.channels.entry(channel.to_string()).or_default();
                        match managed.add_flags_from_chanserv(&notice) {
                            Ok(_) => {}
                            Err(e) => {
                                dbg!(e);
                            }
                        }
                    }
                }
            }
        };
    }
}
