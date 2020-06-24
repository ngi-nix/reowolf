use super::*;

struct MonitoredReader<R: Read> {
    bytes: usize,
    r: R,
}
#[derive(Debug)]
enum TryRecyAnyError {
    Timeout,
    PollFailed,
    EndpointError { error: EndpointError, index: usize },
}

/////////////////////

impl Endpoint {
    pub fn try_recv<T: serde::de::DeserializeOwned>(&mut self) -> Result<Option<T>, EndpointError> {
        use EndpointError::*;
        // populate inbox as much as possible
        'read_loop: loop {
            match self.stream.read_to_end(&mut self.inbox) {
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break 'read_loop,
                Ok(0) => break 'read_loop,
                Ok(_) => (),
                Err(_e) => return Err(BrokenEndpoint),
            }
        }
        let mut monitored = MonitoredReader::from(&self.inbox[..]);
        match bincode::deserialize_from(&mut monitored) {
            Ok(msg) => {
                let msg_size = monitored.bytes_read();
                self.inbox.drain(0..(msg_size.try_into().unwrap()));
                Ok(Some(msg))
            }
            Err(e) => match *e {
                bincode::ErrorKind::Io(k) if k.kind() == std::io::ErrorKind::UnexpectedEof => {
                    Ok(None)
                }
                _ => Err(MalformedMessage),
                // println!("SERDE ERRKIND {:?}", e);
                // Err(MalformedMessage)
            },
        }
    }
    pub fn send<T: serde::ser::Serialize>(&mut self, msg: &T) -> Result<(), EndpointError> {
        bincode::serialize_into(&mut self.stream, msg).map_err(|_| EndpointError::BrokenEndpoint)
    }
}

impl EndpointManager {
    pub fn send_to_setup(&mut self, index: usize, msg: &Msg) -> Result<(), ConnectError> {
        let endpoint = &mut self.endpoint_exts[index].endpoint;
        endpoint.send(msg).map_err(|err| {
            ConnectError::EndpointSetupError(endpoint.stream.local_addr().unwrap(), err)
        })
    }
    pub fn send_to(&mut self, index: usize, msg: &Msg) -> Result<(), EndpointError> {
        self.endpoint_exts[index].endpoint.send(msg)
    }
    pub fn try_recv_any_comms(
        &mut self,
        deadline: Option<Instant>,
    ) -> Result<Option<(usize, Msg)>, SyncError> {
        use {SyncError as Se, TryRecyAnyError as Trae};
        match self.try_recv_any(deadline) {
            Ok(tup) => Ok(Some(tup)),
            Err(Trae::Timeout) => Ok(None),
            Err(Trae::PollFailed) => Err(Se::PollFailed),
            Err(Trae::EndpointError { error, index }) => Err(Se::BrokenEndpoint(index)),
        }
    }
    pub fn try_recv_any_setup(
        &mut self,
        deadline: Option<Instant>,
    ) -> Result<(usize, Msg), ConnectError> {
        use {ConnectError as Ce, TryRecyAnyError as Trae};
        self.try_recv_any(deadline).map_err(|err| match err {
            Trae::Timeout => Ce::Timeout,
            Trae::PollFailed => Ce::PollFailed,
            Trae::EndpointError { error, index } => Ce::EndpointSetupError(
                self.endpoint_exts[index].endpoint.stream.local_addr().unwrap(),
                error,
            ),
        })
    }
    fn try_recv_any(&mut self, deadline: Option<Instant>) -> Result<(usize, Msg), TryRecyAnyError> {
        use TryRecyAnyError::*;
        // 1. try messages already buffered
        if let Some(x) = self.undelayed_messages.pop() {
            return Ok(x);
        }
        loop {
            // 2. try read a message from an endpoint that raised an event with poll() but wasn't drained
            while let Some(index) = self.polled_undrained.pop() {
                let endpoint = &mut self.endpoint_exts[index].endpoint;
                if let Some(msg) =
                    endpoint.try_recv().map_err(|error| EndpointError { error, index })?
                {
                    if !endpoint.inbox.is_empty() {
                        // there may be another message waiting!
                        self.polled_undrained.insert(index);
                    }
                    return Ok((index, msg));
                }
            }
            // 3. No message yet. Do we have enough time to poll?
            let remaining = if let Some(deadline) = deadline {
                Some(deadline.checked_duration_since(Instant::now()).ok_or(Timeout)?)
            } else {
                None
            };
            self.poll.poll(&mut self.events, remaining).map_err(|_| PollFailed)?;
            for event in self.events.iter() {
                let Token(index) = event.token();
                self.polled_undrained.insert(index);
            }
            self.events.clear();
        }
    }
    pub fn undelay_all(&mut self) {
        if self.undelayed_messages.is_empty() {
            // fast path
            std::mem::swap(&mut self.delayed_messages, &mut self.undelayed_messages);
            return;
        }
        // slow path
        self.undelayed_messages.extend(self.delayed_messages.drain(..));
    }
}
impl Debug for Endpoint {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        f.debug_struct("Endpoint").field("inbox", &self.inbox).finish()
    }
}
impl<R: Read> From<R> for MonitoredReader<R> {
    fn from(r: R) -> Self {
        Self { r, bytes: 0 }
    }
}
impl<R: Read> MonitoredReader<R> {
    pub fn bytes_read(&self) -> usize {
        self.bytes
    }
}
impl<R: Read> Read for MonitoredReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, std::io::Error> {
        let n = self.r.read(buf)?;
        self.bytes += n;
        Ok(n)
    }
}

impl Into<Msg> for SetupMsg {
    fn into(self) -> Msg {
        Msg::SetupMsg(self)
    }
}
