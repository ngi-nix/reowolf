#include <time.h>
#include "../../reowolf.h"
#include "../utility.c"
int main(int argc, char** argv) {
	int i, forwards, msglen;
	forwards = atoi(argv[1]);
	msglen = atoi(argv[2]);
	printf("forwards %d, msglen %d\n", forwards, msglen);
	unsigned char pdl[] = ""; 
	Arc_ProtocolDescription * pd = protocol_description_parse(pdl, sizeof(pdl)-1);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	char logpath[] = "./bench_8.txt";
	Connector * c = connector_new_logging(pd, logpath, sizeof(logpath)-1);

	PortId native_putter, native_getter;
	connector_add_port_pair(c, &native_putter, &native_getter);
	for (i=0; i<forwards; i++) {
		PortId putter, getter;
		connector_add_port_pair(c, &putter, &getter);
		// native ports: {native_putter, native_getter, putter, getter}
		char ident[] = "forward"; // defined in reowolf's stdlib 
		// thread a forward component onto native_tail
		connector_add_component(c, ident, sizeof(ident)-1, (PortId[]){native_getter, putter}, 2);
		// native ports: {native_putter, getter}
		printf("Error str `%s`\n", reowolf_error_peek(NULL));
		native_getter = getter;
	}
	connector_connect(c, -1);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	
	char * msg = malloc(msglen);
	memset(msg, 42, msglen);
	
	clock_t begin = clock();
	for (i=0; i<100000; i++) {
		connector_put_bytes(c, native_putter, msg, msglen);
		connector_get(c, native_getter);
		connector_sync(c, -1);
	}
	clock_t end = clock();
	double time_spent = (double)(end - begin) / CLOCKS_PER_SEC;
	printf("Time taken: %f\n", time_spent);
	return 0;
}