#include <time.h>
#include "../../reowolf.h"
#include "../utility.c"
int main(int argc, char** argv) {
	int i, port_pairs;
	port_pairs = atoi(argv[1]);
	printf("Port pairs: %d\n", port_pairs);

	Arc_ProtocolDescription * pd = protocol_description_parse("", 0);
	char logpath[] = "./bench_3.txt";
	Connector * c = connector_new_logging(pd, logpath, sizeof(logpath)-1);
	for (i=0; i<port_pairs; i++) {
		FfiSocketAddr addr = {{127, 0, 0, 1}, (unsigned short)(7000 + i) };
		printf("added port pair %d\n", addr.port);
		connector_add_net_port(c, NULL, addr, Polarity_Putter, EndpointPolarity_Active);
		connector_add_net_port(c, NULL, addr, Polarity_Getter, EndpointPolarity_Passive);
	}
	connector_connect(c, -1);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	
	clock_t begin = clock();
	for (i=0; i<1000000; i++) {
		connector_sync(c, -1);
	}
	clock_t end = clock();
	double time_spent = (double)(end - begin) / CLOCKS_PER_SEC;
	printf("Time taken: %f\n", time_spent);
	return 0;
}