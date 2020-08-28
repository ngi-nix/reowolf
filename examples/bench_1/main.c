#include "../../reowolf.h"
#include "../utility.c"
int main(int argc, char** argv) {
	Arc_ProtocolDescription * pd = protocol_description_parse("", 0);
	char logpath[] = "./bench_1.txt";
	Connector * c = connector_new_logging(pd, logpath, sizeof(logpath)-1);
	rw_err_peek(c);
	
	PortId putter, getter;
	FfiSocketAddr local_addr = {{0, 0, 0, 0}, 8000};
	FfiSocketAddr peer_addr =  {{8, 8, 8, 1}, 8001};
	rw_err_peek(c);
	connector_add_udp_mediator_component(c, &putter, &getter, local_addr, peer_addr);
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
		
		// ... reach new consistent state within 1000ms deadline.
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