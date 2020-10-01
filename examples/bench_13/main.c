#include <time.h>
#include "../../reowolf.h"
#include "../utility.c"
int main(int argc, char** argv) {
	int i, msglen, inside, total;
	char * transport;
	transport = argv[1];
	msglen = atoi(argv[2]);
	inside = atoi(argv[3]);
	total = atoi(argv[4]);
	printf("transport `%s`, msglen %d, inside %d, total %d\n",
		transport, msglen, inside, total);
	unsigned char pdl[] = ""; 
	Arc_ProtocolDescription * pd = protocol_description_parse(pdl, sizeof(pdl)-1);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	char logpath[] = "./bench_13.txt";
	Connector * c = connector_new_logging(pd, logpath, sizeof(logpath)-1);

	PortId native_putter, native_getter;
	char ident[] = "sync"; // defined in reowolf's stdlib 
	connector_add_port_pair(c, &native_putter, &native_getter);
	for (i=0; i<inside; i++) {
		// create a forward linked in the ring
		PortId putter, getter;
		if(0 == strcmp(transport, "mem")) {
			connector_add_port_pair(c, &putter, &getter);
		} else if(0 == strcmp(transport, "localhost")) {
			FfiSocketAddr addr = {{127, 0, 0, 1}, i+7000};
			connector_add_net_port(c, &putter, addr, Polarity_Putter, EndpointPolarity_Active);
			connector_add_net_port(c, &getter, addr, Polarity_Getter, EndpointPolarity_Passive);
		} else {
			printf("BAD TRANSPORT!\n");
			exit(1);
		}
		
		// native ports: {native_putter, native_getter, putter, getter}
		// thread a forward component onto native_tail
		connector_add_component(c, ident, sizeof(ident)-1, (PortId[]){native_getter, putter}, 2);
		// native ports: {native_putter, getter}
		printf("Error str `%s`\n", reowolf_error_peek(NULL));
		native_getter = getter;
	}
	for (i=inside; i<total; i++) {
		// create a forward linked to itself
		PortId putter, getter;
		connector_add_port_pair(c, &putter, &getter);
		connector_add_component(c, ident, sizeof(ident)-1, (PortId[]){getter, putter}, 2);
		printf("Error str `%s`\n", reowolf_error_peek(NULL));
	}
	connector_connect(c, -1);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	
	char * msg = malloc(msglen);
	memset(msg, msglen, 42);
	
	clock_t begin = clock();
	for (i=0; i<100000; i++) {
		connector_put_bytes(c, native_putter, msg, msglen);
		connector_get(c, native_getter);
		connector_sync(c, -1);
	}
	clock_t end = clock();
	double time_spent = (double)(end - begin) / CLOCKS_PER_SEC;
	printf("Time taken: %f\n", time_spent);
	free(msg);
	return 0;
}