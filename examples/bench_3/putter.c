#include "../../reowolf.h"
#include "../utility.c"
int main(int argc, char** argv) {
	Arc_ProtocolDescription * pd = protocol_description_parse("", 0);
	char logpath[] = "./3_16_putter.txt";
	Connector * c = connector_new_logging_with_id(pd, logpath, sizeof(logpath)-1, 1);
	rw_err_peek(c);
	
	PortId putter;
	FfiSocketAddr addr = {{127, 0, 0, 1}, 8001};
	rw_err_peek(c);
	connector_add_net_port(c, &putter, addr, Polarity_Putter, EndpointPolarity_Active);
	connector_connect(c, -1);
	rw_err_peek(c);
	
	// Prepare a message to send
	size_t msg_len = 16;
	char * msg_ptr = malloc(msg_len);
	memset(msg_ptr, 42, msg_len);
	
	int i;
	for(i=0; i<10; i++) {
		connector_put_bytes(c, putter, msg_ptr, msg_len);
		rw_err_peek(c);
		connector_sync(c, -1);
		rw_err_peek(c);
	}
	
	printf("Exiting\n");
	protocol_description_destroy(pd);
	connector_destroy(c);
	free(msg_ptr);
	sleep(1.0);
	return 0;
}