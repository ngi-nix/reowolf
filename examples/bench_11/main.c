#include <time.h>
#include "../../reowolf.h"
#include "../utility.c"
int main(int argc, char** argv) {
	int i, j, forwards, num_options, correct_index;
	forwards = atoi(argv[1]);
	num_options = atoi(argv[2]);
	correct_index = atoi(argv[3]);
	printf("forwards %d, num_options %d, correct_index %d\n",
		forwards, num_options, correct_index);
	unsigned char pdl[] = 
	"primitive recv_zero(in a) {  "
	"    while(true) synchronous {"
	"        msg m = get(a);      "
	"        assert(m[0] == 0);   "
	"    }                        "
	"}                            "
	; 
	Arc_ProtocolDescription * pd = protocol_description_parse(pdl, sizeof(pdl)-1);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	char logpath[] = "./bench_11.txt";
	Connector * c = connector_new_logging(pd, logpath, sizeof(logpath)-1);

	PortId native_putter, native_getter;
	connector_add_port_pair(c, &native_putter, &native_getter);
	for (i=0; i<forwards; i++) {
		// create a forward to tail of chain
		PortId putter, getter;
		connector_add_port_pair(c, &putter, &getter);
		// native ports: {native_putter, native_getter, putter, getter}
		// thread a forward component onto native_tail
		char ident[] = "forward";
		connector_add_component(c, ident, sizeof(ident)-1, (PortId[]){native_getter, putter}, 2);
		// native ports: {native_putter, getter}
		printf("Error str `%s`\n", reowolf_error_peek(NULL));
		native_getter = getter;
	}
	// add "recv_zero" on end of chain
	char ident[] = "recv_zero";
	connector_add_component(c, ident, sizeof(ident)-1, &native_getter, 1);
	connector_connect(c, -1);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	
	clock_t begin = clock();
	char msg = 0;
	for (i=0; i<10000; i++) {
		for(j=0; j<num_options; j++) {
			msg = j==correct_index ? 0 : 1;
			connector_put_bytes(c, native_putter, &msg, 1);
			if(j+1 < num_options) {
				connector_next_batch(c);
			}
		}	
		connector_sync(c, -1);	
	}
	clock_t end = clock();
	double time_spent = (double)(end - begin) / CLOCKS_PER_SEC;
	printf("Time taken: %f\n", time_spent);
	return 0;
}