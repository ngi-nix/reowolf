#include <time.h>
#include "../../reowolf.h"
#include "../utility.c"
int main(int argc, char** argv) {
	// all outward connections are ACTIVE to localhost
	// use tcp_rendezvous
	int i, j, cid, min_putter, min_getter, ports_tot, ports_used;
	char do_puts, do_gets;
	cid = atoi(argv[1]);
	min_putter = atoi(argv[2]);
	min_getter = atoi(argv[3]);
	ports_tot = atoi(argv[4]);
	ports_used = atoi(argv[5]);
	do_puts = argv[6][0]; // 't' or 'f'
	do_gets = argv[7][0];
	printf("cid %d, min_putter %d, min_getter %d, ports_tot %d, ports_used %d, do_puts %c, do_gets %c\n",
		cid, min_putter, min_getter, ports_tot, ports_used, do_puts, do_gets);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));

	unsigned char pdl[] = "";
	Arc_ProtocolDescription * pd = protocol_description_parse(pdl, sizeof(pdl)-1);
	Connector * c = connector_new_with_id(pd, cid);
	PortId putters[ports_tot], getters[ports_tot];
	for(i=0; i<ports_tot; i++) {
		connector_add_net_port(c, &putters[i],
			(FfiSocketAddr){{127, 0, 0, 1}, min_putter+i},
			Polarity_Putter, EndpointPolarity_Active);
		connector_add_net_port(c, &getters[i],
			(FfiSocketAddr){{127, 0, 0, 1}, min_getter+i},
			Polarity_Getter, EndpointPolarity_Active);
	}
	connector_connect(c, -1);
	printf("connect ok!\n");
	
	clock_t begin = clock();
	char msg[] = "Hello, world!";
	for (i=0; i<1000; i++) {
		for(j=0; j<ports_used; j++) {
			if(do_gets=='y') connector_get(c, getters[j]);
			if(do_puts=='y') connector_put_bytes(c, putters[j], msg, sizeof(msg)-1);
		}
		connector_sync(c, -1);
	}
	clock_t end = clock();
	double time_spent = (double)(end - begin) / CLOCKS_PER_SEC;
	printf("Time taken: %f\n", time_spent);
	return 0;
}