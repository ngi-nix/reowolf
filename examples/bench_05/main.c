#include <time.h>
#include "../../reowolf.h"
#include "../utility.c"
int main(int argc, char** argv) {
	int i, port_pairs, proto_components;
	port_pairs = atoi(argv[1]);
	proto_components = atoi(argv[2]);
	printf("port_pairs %d, proto_components: %d\n", port_pairs, proto_components);

	const unsigned char pdl[] = 
	"primitive trivial_loop() {   "
	"    while(true) synchronous{}"
	"}                            "
	;
	Arc_ProtocolDescription * pd = protocol_description_parse(pdl, sizeof(pdl)-1);
	char logpath[] = "./bench_5.txt";
	Connector * c = connector_new_logging(pd, logpath, sizeof(logpath)-1);
	for (i=0; i<port_pairs; i++) {
		connector_add_port_pair(c, NULL, NULL);
	}
	for (i=0; i<proto_components; i++) {
		char ident[] = "trivial_loop";
		connector_add_component(c, ident, sizeof(ident)-1, NULL, 0);
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