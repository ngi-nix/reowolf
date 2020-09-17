#include "../../reowolf.h"
#include "../utility.c"
int main(int argc, char** argv) {
	Arc_ProtocolDescription * pd = protocol_description_parse("", 0);
	char logpath[] = "./3_16_getter.txt";
	Connector * c = connector_new_logging_with_id(pd, logpath, sizeof(logpath)-1, 0);
	rw_err_peek(c);
	
	PortId getter;
	FfiSocketAddr addr = {{127, 0, 0, 1}, 8001};
	rw_err_peek(c);
	connector_add_net_port(c, &getter, addr, Polarity_Getter, EndpointPolarity_Passive);
	connector_connect(c, -1);
	rw_err_peek(c);
	
	int i;
	for(i=0; i<10; i++) {
		connector_get(c, getter);
		rw_err_peek(c);
		connector_sync(c, -1);
		rw_err_peek(c);
	}
	
	printf("Exiting\n");
	protocol_description_destroy(pd);
	connector_destroy(c);
	sleep(1.0);
	return 0;
}