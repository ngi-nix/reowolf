/* This example demonstrates:
- connector API can create UDP mediator components, each given a BIND and CONNECT socket address,
  which communicates via a returned putter and getter port pair.
- messages passed to the UDP mediator are forwarded as UDP datagrams into the network.
*/
#include <stdio.h>
#include <string.h>
#include "../../reowolf.h"

int main(int argc, char** argv) {
  // --- setup ---
  Arc_ProtocolDescription * pd = protocol_description_parse("", 0);
  Connector * c = connector_new(pd);
  PortId putter_a, putter_b;
  FfiSocketAddr addresses[4] = {
    {{127, 0, 0, 1}, 8000},
    {{127, 0, 0, 1}, 8001},
    {{127, 0, 0, 1}, 8002},
    {{127, 0, 0, 1}, 8003},
  };
  
  // putter_a to UDP mediator (getter id discarded)
  // with local addresses[0] and peer addresses[1] 
  connector_add_udp_mediator_component(c, &putter_a, NULL, addresses[1], addresses[0]);
  connector_add_udp_mediator_component(c, &putter_b, NULL, addresses[3], addresses[2]);
  connector_connect(c, -1);
  
  // --- communication --- 
  connector_put_bytes(c, putter_a, "hello, socket!", 14);
  connector_put_bytes(c, putter_b, "hello, pseudo-socket!", 21);
  connector_sync(c, -1);
  
  // --- cleanup ---
  protocol_description_destroy(pd);
  connector_destroy(c);
  return 0;
}
