#include <time.h>
#include "../../reowolf.h"
#include "../utility.c"
int main(int argc, char** argv) {
	int i, self_syncs;
	self_syncs = atoi(argv[1]);
	printf("self_syncs %d\n", self_syncs);
	unsigned char pdl[] = ""; 
	Arc_ProtocolDescription * pd = protocol_description_parse(pdl, sizeof(pdl)-1);
	char logpath[] = "./bench_6.txt";
	Connector * c = connector_new_logging(pd, logpath, sizeof(logpath)-1);
	for (i=0; i<self_syncs; i++) {
		PortId putter, getter;
		connector_add_port_pair(c, &putter, &getter);
		char ident[] = "sync"; // defined in reowolf's stdlib 
		connector_add_component(c, ident, sizeof(ident)-1, (PortId[]){getter, putter}, 2);
		printf("Error str `%s`\n", reowolf_error_peek(NULL));
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