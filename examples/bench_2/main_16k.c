#include "../../reowolf.h"
#include "../utility.c"
int main(int argc, char** argv) {
	Arc_ProtocolDescription * pd = protocol_description_parse("", 0);
	char logpath[] = "./2_16k.txt";
	Connector * c = connector_new_logging(pd, logpath, sizeof(logpath)-1);
	rw_err_peek(c);
	
	PortId putter, getter;
	rw_err_peek(c);
	connector_add_port_pair(c, &putter, &getter);
	connector_connect(c, -1);
	rw_err_peek(c);
	
	// Prepare a message to send
	size_t msg_len = 16000;
	char * msg_ptr = malloc(msg_len);
	memset(msg_ptr, 42, msg_len);
	
	int i;
	for(i=0; i<10; i++) {
		connector_put_bytes(c, putter, msg_ptr, msg_len);
		connector_get(c, getter);
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