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
impl NetEndpoint {
    fn bincode_opts() -> impl bincode::config::Options {
        bincode::config::DefaultOptions::default()
    }
    pub(super) fn try_recv<T: serde::de::DeserializeOwned>(
        &mut self,
        logger: &mut dyn Logger,
    ) -> Result<Option<T>, EndpointError> {
        use EndpointError as Ee;
        // populate inbox as much as possible
        let before_len = self.inbox.len();
        'read_loop: loop {
            let res = self.stream.read_to_end(&mut self.inbox);
            match res {
                Err(e) if would_block(&e) => break 'read_loop,
                Ok(0) => break 'read_loop,
                Ok(_) => (),
                Err(_e) => return Err(Ee::BrokenEndpoint),
            }
        }
        endptlog!(
            logger,
            "Inbox bytes [{:x?}| {:x?}]",
            DenseDebugHex(&self.inbox[..before_len]),
            DenseDebugHex(&self.inbox[before_len..]),
        );
        let mut monitored = MonitoredReader::from(&self.inbox[..]);
        use bincode::config::Options;
        match Self::bincode_opts().deserialize_from(&mut monitored) {
            Ok(msg) => {
                let msg_size = monitored.bytes_read();
                self.inbox.drain(0..(msg_size.try_into().unwrap()));
                endptlog!(
                    logger,
                    "Yielding msg. Inbox len {}-{}=={}: [{:?}]",
                    self.inbox.len() + msg_size,
                    msg_size,
                    self.inbox.len(),
                    DenseDebugHex(&self.inbox[..]),
                );
                Ok(Some(msg))
            }
            Err(e) => match *e {
                bincode::ErrorKind::Io(k) if k.kind() == std::io::ErrorKind::UnexpectedEof => {
                    Ok(None)
                }
                _ => Err(Ee::MalformedMessage),
            },
        }
    }
    pub(super) fn send<T: serde::ser::Serialize>(&mut self, msg: &T) -> Result<(), EndpointError> {
        use bincode::config::Options;
        use EndpointError as Ee;
        Self::bincode_opts().serialize_into(&mut self.stream, msg).map_err(|_| Ee::BrokenEndpoint)
    }
}

impl EndpointManager {
    pub(super) fn index_iter(&self) -> Range<usize> {
        0..self.num_net_endpoints()
    }
    pub(super) fn num_net_endpoints(&self) -> usize {
        self.net_endpoint_exts.len()
    }
    pub(super) fn send_to_comms(
        &mut self,
        index: usize,
        msg: &Msg,
    ) -> Result<(), UnrecoverableSyncError> {
        use UnrecoverableSyncError as Use;
        let net_endpoint = &mut self.net_endpoint_exts[index].net_endpoint;
        net_endpoint.send(msg).map_err(|_| Use::BrokenEndpoint(index))
    }
    pub(super) fn send_to_setup(&mut self, index: usize, msg: &Msg) -> Result<(), ConnectError> {
        let net_endpoint = &mut self.net_endpoint_exts[index].net_endpoint;
        net_endpoint.send(msg).map_err(|err| {
            ConnectError::EndpointSetupError(net_endpoint.stream.local_addr().unwrap(), err)
        })
    }
    pub(super) fn try_recv_any_comms(
        &mut self,
        logger: &mut dyn Logger,
        deadline: Option<Instant>,
    ) -> Result<Option<(usize, Msg)>, UnrecoverableSyncError> {
        use {TryRecyAnyError as Trae, UnrecoverableSyncError as Use};
        match self.try_recv_any(logger, deadline) {
            Ok(tup) => Ok(Some(tup)),
            Err(Trae::Timeout) => Ok(None),
            Err(Trae::PollFailed) => Err(Use::PollFailed),
            Err(Trae::EndpointError { error: _, index }) => Err(Use::BrokenEndpoint(index)),
        }
    }
    pub(super) fn try_recv_any_setup(
        &mut self,
        logger: &mut dyn Logger,
        deadline: Option<Instant>,
    ) -> Result<(usize, Msg), ConnectError> {
        use {ConnectError as Ce, TryRecyAnyError as Trae};
        self.try_recv_any(logger, deadline).map_err(|err| match err {
            Trae::Timeout => Ce::Timeout,
            Trae::PollFailed => Ce::PollFailed,
            Trae::EndpointError { error, index } => Ce::EndpointSetupError(
                self.net_endpoint_exts[index].net_endpoint.stream.local_addr().unwrap(),
                error,
            ),
        })
    }
    fn try_recv_any(
        &mut self,
        logger: &mut dyn Logger,
        deadline: Option<Instant>,
    ) -> Result<(usize, Msg), TryRecyAnyError> {
        use TryRecyAnyError as Trea;
        // 1. try messages already buffered
        if let Some(x) = self.undelayed_messages.pop() {
            endptlog!(logger, "RECV undelayed_msg {:?}", &x);
            return Ok(x);
        }
        loop {
            // 2. try read a message from an endpoint that raised an event with poll() but wasn't drained
            while let Some(index) = self.polled_undrained.pop() {
                let net_endpoint = &mut self.net_endpoint_exts[index].net_endpoint;
                if let Some(msg) = net_endpoint
                    .try_recv(logger)
                    .map_err(|error| Trea::EndpointError { error, index })?
                {
                    endptlog!(logger, "RECV polled_undrained {:?}", &msg);
                    if !net_endpoint.inbox.is_empty() {
                        // there may be another message waiting!
                        self.polled_undrained.insert(index);
                    }
                    return Ok((index, msg));
                }
            }
            // 3. No message yet. Do we have enough time to poll?
            let remaining = if let Some(deadline) = deadline {
                Some(deadline.checked_duration_since(Instant::now()).ok_or(Trea::Timeout)?)
            } else {
                None
            };
            self.poll.poll(&mut self.events, remaining).map_err(|_| Trea::PollFailed)?;
            for event in self.events.iter() {
                let Token(index) = event.token();
                self.polled_undrained.insert(index);
                endptlog!(
                    logger,
                    "RECV poll event {:?} for endpoint index {:?}. undrained: {:?}",
                    &event,
                    index,
                    self.polled_undrained.iter()
                );
            }
            self.events.clear();
        }
    }
    pub(super) fn undelay_all(&mut self) {
        if self.undelayed_messages.is_empty() {
            // fast path
            std::mem::swap(&mut self.delayed_messages, &mut self.undelayed_messages);
            return;
        }
        // slow path
        self.undelayed_messages.extend(self.delayed_messages.drain(..));
    }
}
impl Debug for NetEndpoint {
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
    pub(super) fn bytes_read(&self) -> usize {
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
