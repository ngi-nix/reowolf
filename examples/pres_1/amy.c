#include "../../reowolf.h"
#include "../utility.c"
int main(int argc, char** argv) {
	char msgbuf[64];
	// ask user what message to send
	size_t msglen = get_user_msg(msgbuf, sizeof(msgbuf));

	// Create a connector, configured with our (trivial) protocol.
	Arc_ProtocolDescription * pd = protocol_description_parse("", 0);
	char logpath[] = "./pres_1_amy.txt";
	Connector * c = connector_new_logging(pd, logpath, sizeof(logpath)-1);
	rw_err_peek(c);
	
	// ... with 1 outgoing network connection
	PortId p0;
	FfiSocketAddr addr = {{127, 0, 0, 1}, 8000};
	connector_add_net_port(c, &p0, addr, Polarity_Putter, EndpointPolarity_Passive);
	rw_err_peek(c);
	
	// Connect with peers (5000ms timeout).
	connector_connect(c, 5000);
	rw_err_peek(c);
	
	// Prepare a message to send
	connector_put_bytes(c, p0, msgbuf, msglen);
	rw_err_peek(c);
	
	// ... reach new consistent state within 1000ms deadline.
	connector_sync(c, 1000);
	rw_err_peek(c);
	
	printf("Exiting\n");
	protocol_description_destroy(pd);
	connector_destroy(c);
	sleep(1.0);
	return 0;
}