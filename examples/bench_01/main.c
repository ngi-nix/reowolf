#include <time.h>
#include "../../reowolf.h"
#include "../utility.c"
int main(int argc, char** argv) {
	Arc_ProtocolDescription * pd = protocol_description_parse("", 0);
	char logpath[] = "./bench_1.txt";
	Connector * c = connector_new_logging(pd, logpath, sizeof(logpath)-1);
	connector_connect(c, -1);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	
	clock_t begin = clock();
	int i;
	for (i=0; i<1000000; i++) {
		connector_sync(c, -1);
	}
	clock_t end = clock();
	double time_spent = (double)(end - begin) / CLOCKS_PER_SEC;
	printf("Time taken: %f\n", time_spent);
	return 0;
}