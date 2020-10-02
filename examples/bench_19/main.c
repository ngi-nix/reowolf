#include <time.h>
#include "../../reowolf.h"
#include "../utility.c"
int main(int argc, char** argv) {
	// bounce off tokyo to my public IP
	int i, j, cid, min_putter, min_getter, ports_tot, ports_used, n_rounds;
	char do_puts, do_gets;
	cid = atoi(argv[1]);
	min_putter = atoi(argv[2]);
	min_getter = atoi(argv[3]);
	ports_tot = atoi(argv[4]);
	ports_used = atoi(argv[5]);
	do_puts = argv[6][0]; // 't' or 'f'
	do_gets = argv[7][0];
	n_rounds = atoi(argv[12]);
	
	// argv 8..12 is PEER_IP
	
	printf("cid %d, min_putter %d, min_getter %d, ports_tot %d, ports_used %d, do_puts %c, do_gets %c, n_rounds %d\n",
		cid, min_putter, min_getter, ports_tot, ports_used, do_puts, do_gets, n_rounds);
	printf("peer_ip %d.%d.%d.%d\n",
		atoi(argv[8]),
		atoi(argv[9]),
		atoi(argv[10]),
		atoi(argv[11]));
	
	printf("Error str `%s`\n", reowolf_error_peek(NULL));

	unsigned char pdl[] = "";
	Arc_ProtocolDescription * pd = protocol_description_parse(pdl, sizeof(pdl)-1);
	Connector * c = connector_new_with_id(pd, cid);
	PortId putters[ports_tot], getters[ports_tot];
	for(i=0; i<ports_tot; i++) {
		connector_add_net_port(c, &putters[i],
			(FfiSocketAddr){
				{atoi(argv[8]),atoi(argv[9]),atoi(argv[10]),atoi(argv[11]),},
				min_putter+i
			},
			Polarity_Putter, EndpointPolarity_Active);
		connector_add_net_port(c, &getters[i],
			(FfiSocketAddr){
				{0,0,0,0},
				min_getter+i
			},
			Polarity_Getter, EndpointPolarity_Passive);
	}
	connector_connect(c, -1);
	printf("connect ok!\n");
	
	clock_t begin = clock();
	char msg[] = "Hello, world!";
	for (i=0; i<n_rounds; i++) {
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