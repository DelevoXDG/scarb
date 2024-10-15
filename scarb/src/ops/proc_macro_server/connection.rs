use super::json_rpc::{Request, Response};
use crossbeam_channel::{Receiver, Sender};
use std::{
    io::{BufRead, Write},
    thread::JoinHandle,
};

pub struct Connection {
    pub sender: Sender<Response>,
    pub receiver: Receiver<Request>,
    _io_threads: IoThreads,
}

impl Connection {
    pub fn new() -> Self {
        let (sender, receiver) = crossbeam_channel::bounded::<Request>(0);
        let (sender2, receiver2) = crossbeam_channel::bounded::<Response>(0);

        let reader = std::thread::spawn(move || {
            let stdin = std::io::stdin();
            let mut stdin = stdin.lock();

            let mut line = String::new();
            stdin.read_line(&mut line).unwrap();

            sender.send(serde_json::from_str(&line).unwrap()).unwrap();
        });

        let writer = std::thread::spawn(move || {
            let stdout = std::io::stdout();
            let mut stdout = stdout.lock();

            for response in receiver2 {
                let res = serde_json::to_vec(&response).unwrap();

                stdout.write_all(&res).unwrap()
            }
        });

        let io_threads = IoThreads { reader, writer };

        Self {
            sender: sender2,
            receiver,
            _io_threads: io_threads,
        }
    }
}

struct IoThreads {
    reader: JoinHandle<()>,
    writer: JoinHandle<()>,
}

impl Drop for IoThreads {
    fn drop(&mut self) {
        // std::mem::take(&mut self.reader).join();
        // self.writer.join();
        //TODO
    }
}
