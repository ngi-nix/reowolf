#include <stdio.h>
#include <string.h>
#include "../../reowolf.h"

int main(int argc, char** argv) {
  // --- setup ---
  Arc_ProtocolDescription * pd = protocol_description_parse("", 0);
  Connector * c = connector_new(pd);
  PortId putter_a, putter_b;
  FfiSocketAddr addresses[4];
  /* (address structure initializations omitted) */
  
  // putter_a to UDP mediator (getter id discarded)
  // with local addresses[0] and peer addresses[1] 
  connector_add_udp_mediator_component(c, &putter_a, NULL, addresses[0], addresses[1]);
  connector_add_udp_mediator_component(c, &putter_b, NULL, addresses[2], addresses[3]);
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
