
#include "../../reowolf.h"
#include "../utility.c"

int main(int argc, char** argv) {
	// Create a connector...
	Arc_ProtocolDescription * pd = protocol_description_parse("", 0);
	char logpath[] = "./pres_1_bob.txt";
	Connector * c = connector_new_logging(pd, logpath, sizeof(logpath)-1);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	
	// ... with 1 outgoing network connection
	PortId p0;
	char addr_str[] = "127.0.0.1:8000";
	connector_add_net_port(
		c, &p0, addr_str, sizeof(addr_str)-1, Polarity_Putter, EndpointPolarity_Passive);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	
	// Connect (5000ms timeout). Begin communication. 
	connector_connect(c, 5000);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	
	// Send a single 2-byte message...
	connector_put_bytes(c, p0, "hi", 2);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	
	// ... and acknowledge receipt within 1000ms.
	connector_sync(c, 1000);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	
	printf("Exiting\n");
	protocol_description_destroy(pd);
	connector_destroy(c);
	sleep(1.0);
	return 0;
}