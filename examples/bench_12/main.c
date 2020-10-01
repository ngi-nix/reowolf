#include <time.h>
#include "../../reowolf.h"
#include "../utility.c"
int main(int argc, char** argv) {
	int i, j, batches;
	batches = atoi(argv[1]);
	printf("batches %d\n", batches);
	unsigned char pdl[] = "";
	Arc_ProtocolDescription * pd = protocol_description_parse(pdl, sizeof(pdl)-1);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	char logpath[] = "./bench_12.txt";
	Connector * c = connector_new_logging(pd, logpath, sizeof(logpath)-1);
	
	connector_connect(c, -1);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	
	clock_t begin = clock();
	char msg = 0;
	for (i=0; i<1000000; i++) {
		for(j=1; j<batches; j++) {
			connector_next_batch(c);
		}
		connector_sync(c, -1);
	}
	clock_t end = clock();
	double time_spent = (double)(end - begin) / CLOCKS_PER_SEC;
	printf("Time taken: %f\n", time_spent);
	return 0;
}