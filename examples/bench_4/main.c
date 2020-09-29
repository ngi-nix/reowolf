#include <time.h>
#include "../../reowolf.h"
#include "../utility.c"
int main(int argc, char** argv) {
	int i, proto_components;
	proto_components = atoi(argv[1]);
	printf("proto_components: %d\n", proto_components);

	const unsigned char pdl[] = 
	"primitive trivial_loop() {   "
	"    while(true) synchronous{}"
	"}                            "
	;
	Arc_ProtocolDescription * pd = protocol_description_parse(pdl, sizeof(pdl)-1);
	char logpath[] = "./bench_4.txt";
	Connector * c = connector_new_logging(pd, logpath, sizeof(logpath)-1);
	for (i=0; i<proto_components; i++) {
		connector_add_component(c, "trivial_loop", 12, NULL, 0);
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