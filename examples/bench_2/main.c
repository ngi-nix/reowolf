#include <time.h>
#include "../../reowolf.h"
#include "../utility.c"
int main(int argc, char** argv) {
	int i;
	Arc_ProtocolDescription * pd = protocol_description_parse("", 0);
	char logpath[] = "./bench_2.txt";
	Connector * c = connector_new_logging(pd, logpath, sizeof(logpath)-1);
	int port_pairs = atoi(argv[1]);
	printf("Port pairs: %d\n", port_pairs);
	for (i=0; i<port_pairs; i++) {
		connector_add_port_pair(c, NULL, NULL);
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