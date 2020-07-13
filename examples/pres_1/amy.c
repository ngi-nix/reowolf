#include "../../reowolf.h"
#include "../utility.c"
int main(int argc, char** argv) {
	// Create a connector...
	Arc_ProtocolDescription * pd = protocol_description_parse("", 0);
	char logpath[] = "./pres_1_amy.txt";
	Connector * c = connector_new_logging(pd, logpath, sizeof(logpath)-1);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	
	// ... with 1 outgoing network connection
	PortId p0;
	char addr_str[] = "127.0.0.1:8000";
	connector_add_net_port(
		c, &p0, addr_str, sizeof(addr_str)-1, Polarity_Getter, EndpointPolarity_Active);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	
	// Connect! Begin communication. 5000ms timeout
	connector_connect(c, 5000);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	
	// Ask to receive a message...
	connector_get(c, p0);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	
	// ... or timeout within 1000ms.
	connector_sync(c, 1000);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	
	// Print the message we received
	size_t msg_len;
	const char * msg_ptr = connector_gotten_bytes(c, p0, &msg_len);
	printf("Got msg `%.*s`\n", msg_len, msg_ptr);
	
	printf("Exiting\n");
	protocol_description_destroy(pd);
	connector_destroy(c);
	sleep(1.0);
	return 0;
}