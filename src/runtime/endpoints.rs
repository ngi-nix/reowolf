use super::*;

enum PollAndPopulateError {
    PollFailed,
    Timeout,
}

struct TryRecvAnyNetError {
    error: NetEndpointError,
    index: usize,
}
/////////////////////
impl NetEndpoint {
    // Returns the bincode configuration the NetEndpoint uses pervasively
    // for configuration on ser/de operations.
    fn bincode_opts() -> impl bincode::config::Options {
        // uses variable-length encoding everywhere; great!
        bincode::config::DefaultOptions::default()
    }

    // Attempt to return some deserializable T-type from
    // the inbox or network stream
    pub(super) fn try_recv<T: serde::de::DeserializeOwned>(
        &mut self,
        logger: &mut dyn Logger,
    ) -> Result<Option<T>, NetEndpointError> {
        use NetEndpointError as Nee;
        // populate inbox with bytes as much as possible (mio::TcpStream is nonblocking)
        let before_len = self.inbox.len();
        'read_loop: loop {
            let res = self.stream.read_to_end(&mut self.inbox);
            match res {
                Err(e) if err_would_block(&e) => break 'read_loop,
                Ok(0) => break 'read_loop,
                Ok(_) => (),
                Err(_e) => return Err(Nee::BrokenNetEndpoint),
            }
        }
        log!(
            @ENDPT,
            logger,
            "Inbox bytes [{:x?}| {:x?}]",
            DenseDebugHex(&self.inbox[..before_len]),
            DenseDebugHex(&self.inbox[before_len..]),
        );
        // Try deserialize from the inbox. `reading_slice' is updated by read()
        // in-place to truncate the read part. In the event of success,
        // the message bytes are contained in the truncated prefix
        let mut reading_slice = self.inbox.as_slice();
        let before_len = reading_slice.len();
        use bincode::config::Options;
        match Self::bincode_opts().deserialize_from(&mut reading_slice) {
            Ok(msg) => {
                let msg_size = before_len - reading_slice.len();
                // inbox[..msg_size] was deserialized into one message!
                self.inbox.drain(..msg_size);
                log!(
                    @ENDPT,
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
                    // Contents of inbox insufficient for deserializing a message
                    Ok(None)
                }
                _ => Err(Nee::MalformedMessage),
            },
        }
    }

    // Send the given serializable type into the stream
    pub(super) fn send<T: serde::ser::Serialize + Debug>(
        &mut self,
        msg: &T,
        io_byte_buffer: &mut IoByteBuffer,
    ) -> Result<(), NetEndpointError> {
        use bincode::config::Options;
        use NetEndpointError as Nee;
        // Create a buffer for our bytes: a slice of the io_byte_buffer
        let mut buf_slice = io_byte_buffer.as_mut_slice();
        // serialize into the slice, truncating as its filled
        Self::bincode_opts().serialize_into(&mut buf_slice, msg).expect("Serialize failed!");
        // written segment is the part missing from buf_slice. Write this as one segment to the TCP stream
        let wrote = IoByteBuffer::CAPACITY - buf_slice.len();
        self.stream
            .write_all(&io_byte_buffer.as_mut_slice()[..wrote])
            .map_err(|_| Nee::BrokenNetEndpoint)?;
        let _ = self.stream.flush();
        Ok(())
    }
}

impl EndpointManager {
    pub(super) fn index_iter(&self) -> Range<usize> {
        0..self.num_net_endpoints()
    }
    pub(super) fn num_net_endpoints(&self) -> usize {
        self.net_endpoint_store.endpoint_exts.len()
    }

    // Setup-phase particular send procedure.
    // Used pervasively, allows for some brevity with the ? operator.
    pub(super) fn send_to_setup(&mut self, index: usize, msg: &Msg) -> Result<(), ConnectError> {
        let net_endpoint = &mut self.net_endpoint_store.endpoint_exts[index].net_endpoint;
        net_endpoint.send(msg, &mut self.io_byte_buffer).map_err(|err| {
            ConnectError::NetEndpointSetupError(net_endpoint.stream.local_addr().unwrap(), err)
        })
    }

    // Communication-phase particular send procedure.
    // Used pervasively, allows for some brevity with the ? operator.
    pub(super) fn send_to_comms(
        &mut self,
        index: usize,
        msg: &Msg,
    ) -> Result<(), UnrecoverableSyncError> {
        use UnrecoverableSyncError as Use;
        let net_endpoint = &mut self.net_endpoint_store.endpoint_exts[index].net_endpoint;
        net_endpoint
            .send(msg, &mut self.io_byte_buffer)
            .map_err(|_| Use::BrokenNetEndpoint { index })
    }

    /// Receive the first message of any kind at all.
    /// Why not return SetupMsg? Because often this message will be forwarded to several others,
    /// and by returning a Msg, it can be serialized in-place (NetEndpoints allow the sending of Msg types!)
    pub(super) fn try_recv_any_setup(
        &mut self,
        logger: &mut dyn Logger,
        deadline: &Option<Instant>,
    ) -> Result<(usize, Msg), ConnectError> {
        // helper function, mapping a TryRecvAnySetup type error
        // into a ConnectError
        fn map_trane(
            trane: TryRecvAnyNetError,
            net_endpoint_store: &EndpointStore<NetEndpointExt>,
        ) -> ConnectError {
            ConnectError::NetEndpointSetupError(
                net_endpoint_store.endpoint_exts[trane.index]
                    .net_endpoint
                    .stream
                    .local_addr()
                    .unwrap(), // stream must already be connected
                trane.error,
            )
        }
        // try yield undelayed net message
        if let Some(tup) = self.undelayed_messages.pop() {
            log!(@ENDPT, logger, "RECV undelayed_msg {:?}", &tup);
            return Ok(tup);
        }
        loop {
            // try recv from some polled undrained NET endpoint
            if let Some(tup) = self
                .try_recv_undrained_net(logger)
                .map_err(|trane| map_trane(trane, &self.net_endpoint_store))?
            {
                return Ok(tup);
            }
            // poll if time remains
            self.poll_and_populate(logger, deadline)?;
        }
    }

    // drops all Setup messages,
    // buffers all future round messages,
    // drops all previous round messages,
    // enqueues all current round SendPayload messages using rctx.getter_push
    // returns the first comm_ctrl_msg encountered
    // only polls until SOME message is enqueued
    pub(super) fn try_recv_any_comms(
        &mut self,
        cu: &mut impl CuUndecided,
        rctx: &mut RoundCtx,
        round_index: usize,
    ) -> Result<CommRecvOk, UnrecoverableSyncError> {
        ///////////////////////////////////////////
        // adds scoped functionality for EndpointManager
        impl EndpointManager {
            // Given some Msg structure in a particular context,
            // return a control message for the current round
            // if its a payload message, buffer it instead
            fn handle_msg(
                &mut self,
                cu: &mut impl CuUndecided,
                rctx: &mut RoundCtx,
                net_index: usize,
                msg: Msg,
                round_index: usize,
                some_message_enqueued: &mut bool,
            ) -> Option<(usize, CommCtrlMsg)> {
                let comm_msg_contents = match msg {
                    Msg::SetupMsg(..) => return None, // discard setup messages
                    Msg::CommMsg(comm_msg) => match comm_msg.round_index.cmp(&round_index) {
                        Ordering::Equal => comm_msg.contents, // ok, keep going
                        Ordering::Less => {
                            // discard this message
                            log!(
                                cu.logger(),
                                "We are in round {}, but msg is for round {}. Discard",
                                comm_msg.round_index,
                                round_index,
                            );
                            return None;
                        }
                        Ordering::Greater => {
                            // "delay" this message, enqueueing it for a future round
                            log!(
                                cu.logger(),
                                "We are in round {}, but msg is for round {}. Buffer",
                                comm_msg.round_index,
                                round_index,
                            );
                            self.delayed_messages.push((net_index, Msg::CommMsg(comm_msg)));
                            return None;
                        }
                    },
                };
                // inspect the contents of this contemporary message, sorting it
                match comm_msg_contents {
                    CommMsgContents::CommCtrl(comm_ctrl_msg) => {
                        // yes! this is a CommCtrlMsg
                        Some((net_index, comm_ctrl_msg))
                    }
                    CommMsgContents::SendPayload(send_payload_msg) => {
                        // Enqueue this payload message
                        // Still not a CommCtrlMsg, so return None
                        let getter =
                            self.net_endpoint_store.endpoint_exts[net_index].getter_for_incoming;
                        rctx.getter_push(getter, send_payload_msg);
                        *some_message_enqueued = true;
                        None
                    }
                }
            }
        }
        use {PollAndPopulateError as Pape, UnrecoverableSyncError as Use};
        ///////////////////////////////////////////
        let mut some_message_enqueued = false;
        // pop undelayed messages, handling them. Return the first CommCtrlMsg popped
        while let Some((net_index, msg)) = self.undelayed_messages.pop() {
            if let Some((net_index, msg)) =
                self.handle_msg(cu, rctx, net_index, msg, round_index, &mut some_message_enqueued)
            {
                return Ok(CommRecvOk::NewControlMsg { net_index, msg });
            }
        }
        loop {
            // drain endpoints of incoming messages (without blocking)
            // return first CommCtrlMsg received
            while let Some((net_index, msg)) = self.try_recv_undrained_net(cu.logger())? {
                if let Some((net_index, msg)) = self.handle_msg(
                    cu,
                    rctx,
                    net_index,
                    msg,
                    round_index,
                    &mut some_message_enqueued,
                ) {
                    return Ok(CommRecvOk::NewControlMsg { net_index, msg });
                }
            }
            // try receive a udp message
            let recv_buffer = self.io_byte_buffer.as_mut_slice();
            while let Some(index) = self.udp_endpoint_store.polled_undrained.pop() {
                let ee = &mut self.udp_endpoint_store.endpoint_exts[index];
                if let Some(bytes_written) = ee.sock.recv(recv_buffer).ok() {
                    // I received a payload!
                    self.udp_endpoint_store.polled_undrained.insert(index);
                    if !ee.received_this_round {
                        let payload = Payload::from(&recv_buffer[..bytes_written]);
                        let port_spec_var = rctx.ips.port_info.spec_var_for(ee.getter_for_incoming);
                        let predicate = Predicate::singleton(port_spec_var, SpecVal::FIRING);
                        rctx.getter_push(
                            ee.getter_for_incoming,
                            SendPayloadMsg { payload, predicate },
                        );
                        some_message_enqueued = true;
                        ee.received_this_round = true;
                    } else {
                        // lose the message!
                    }
                }
            }
            if some_message_enqueued {
                return Ok(CommRecvOk::NewPayloadMsgs);
            }
            // poll if time remains
            match self.poll_and_populate(cu.logger(), &rctx.deadline) {
                Ok(()) => {} // continue looping
                Err(Pape::Timeout) => return Ok(CommRecvOk::TimeoutWithoutNew),
                Err(Pape::PollFailed) => return Err(Use::PollFailed),
            }
        }
    }

    // Try receive some message from any net endpoint without blocking
    fn try_recv_undrained_net(
        &mut self,
        logger: &mut dyn Logger,
    ) -> Result<Option<(usize, Msg)>, TryRecvAnyNetError> {
        while let Some(index) = self.net_endpoint_store.polled_undrained.pop() {
            let net_endpoint = &mut self.net_endpoint_store.endpoint_exts[index].net_endpoint;
            if let Some(msg) = net_endpoint
                .try_recv(logger)
                .map_err(|error| TryRecvAnyNetError { error, index })?
            {
                log!(@ENDPT, logger, "RECV polled_undrained {:?}", &msg);
                if !net_endpoint.inbox.is_empty() {
                    // there may be another message waiting!
                    self.net_endpoint_store.polled_undrained.insert(index);
                }
                return Ok(Some((index, msg)));
            }
        }
        Ok(None)
    }

    // Poll the network, raising `polled_undrained` flags for endpoints
    // as they receive events.
    fn poll_and_populate(
        &mut self,
        logger: &mut dyn Logger,
        deadline: &Option<Instant>,
    ) -> Result<(), PollAndPopulateError> {
        use PollAndPopulateError as Pape;
        // No message yet. Do we have enough time to poll?
        let remaining = if let Some(deadline) = deadline {
            Some(deadline.checked_duration_since(Instant::now()).ok_or(Pape::Timeout)?)
        } else {
            None
        };
        // Yes we do! Poll with remaining time as poll deadline
        self.poll.poll(&mut self.events, remaining).map_err(|_| Pape::PollFailed)?;
        for event in self.events.iter() {
            match TokenTarget::from(event.token()) {
                TokenTarget::NetEndpoint { index } => {
                    self.net_endpoint_store.polled_undrained.insert(index);
                    log!(
                        @ENDPT,
                        logger,
                        "RECV poll event {:?} for NET endpoint index {:?}. undrained: {:?}",
                        &event,
                        index,
                        self.net_endpoint_store.polled_undrained.iter()
                    );
                }
                TokenTarget::UdpEndpoint { index } => {
                    self.udp_endpoint_store.polled_undrained.insert(index);
                    log!(
                        @ENDPT,
                        logger,
                        "RECV poll event {:?} for UDP endpoint index {:?}. undrained: {:?}",
                        &event,
                        index,
                        self.udp_endpoint_store.polled_undrained.iter()
                    );
                }
            }
        }
        self.events.clear();
        Ok(())
    }

    // Move all delayed messages to undelayed, making it possible to yield them
    pub(super) fn undelay_all(&mut self) {
        if self.undelayed_messages.is_empty() {
            // fast path
            std::mem::swap(&mut self.delayed_messages, &mut self.undelayed_messages);
            return;
        }
        // slow path
        self.undelayed_messages.extend(self.delayed_messages.drain(..));
    }

    // End the synchronous round for the udp endpoints given the round decision
    pub(super) fn udp_endpoints_round_end(
        &mut self,
        logger: &mut dyn Logger,
        decision: &Decision,
    ) -> Result<(), UnrecoverableSyncError> {
        // retain received_from_this_round for use in pseudo_socket_api::recv_from
        log!(
            logger,
            "Ending round for {} udp endpoints",
            self.udp_endpoint_store.endpoint_exts.len()
        );
        use UnrecoverableSyncError as Use;
        if let Decision::Success(solution_predicate) = decision {
            // Similar to a native component, we commit the branch of the component
            // consistent with the predicate decided upon, making its effects visible
            // to the world outside the connector's internals.
            // In this case, this takes the form of emptying the component's outbox buffer,
            // actually sending payloads 'on the wire' as UDP messages.
            for (index, ee) in self.udp_endpoint_store.endpoint_exts.iter_mut().enumerate() {
                'outgoing_loop: for (payload_predicate, payload) in ee.outgoing_payloads.drain() {
                    if payload_predicate.assigns_subset(solution_predicate) {
                        ee.sock.send(payload.as_slice()).map_err(|e| {
                            println!("{:?}", e);
                            Use::BrokenUdpEndpoint { index }
                        })?;
                        log!(
                            logger,
                            "Sent payload {:?} with pred {:?} through Udp endpoint {}",
                            &payload,
                            &payload_predicate,
                            index
                        );
                        // send at most one payload per endpoint per round
                        break 'outgoing_loop;
                    }
                }
                ee.received_this_round = false;
            }
        }
        Ok(())
    }
}
impl Debug for NetEndpoint {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        struct DebugStream<'a>(&'a TcpStream);
        impl Debug for DebugStream<'_> {
            fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
                f.debug_struct("Endpoint")
                    .field("local_addr", &self.0.local_addr())
                    .field("peer_addr", &self.0.peer_addr())
                    .finish()
            }
        }
        f.debug_struct("Endpoint")
            .field("inbox", &self.inbox)
            .field("stream", &DebugStream(&self.stream))
            .finish()
    }
}
impl Into<Msg> for SetupMsg {
    fn into(self) -> Msg {
        Msg::SetupMsg(self)
    }
}
impl From<PollAndPopulateError> for ConnectError {
    fn from(pape: PollAndPopulateError) -> ConnectError {
        use {ConnectError as Ce, PollAndPopulateError as Pape};
        match pape {
            Pape::PollFailed => Ce::PollFailed,
            Pape::Timeout => Ce::Timeout,
        }
    }
}
impl From<TryRecvAnyNetError> for UnrecoverableSyncError {
    fn from(trane: TryRecvAnyNetError) -> UnrecoverableSyncError {
        let TryRecvAnyNetError { index, .. } = trane;
        UnrecoverableSyncError::BrokenNetEndpoint { index }
    }
}
