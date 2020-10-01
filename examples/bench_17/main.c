#include <time.h>
#include "../../reowolf.h"
#include "../utility.c"
#define N 5
int main(int argc, char** argv) {
	int i, cid, min_pid, msgs;
	cid = atoi(argv[1]);
	min_pid = atoi(argv[2]);
	char role = argv[3][0]; // 'h' for head, 'i' for inner, 't' for tail, 's' for singleton
	msgs = atoi(argv[4]);
	printf("cid %d, min_pid %d, role='%c', msgs %d\n",
		cid, min_pid, role, msgs);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));

	unsigned char pdl[] = "";
	Arc_ProtocolDescription * pd = protocol_description_parse(pdl, sizeof(pdl)-1);
	Connector * c = connector_new_with_id(pd, cid);
	PortId putters[N], getters[N];
	FfiSocketAddr addr = {{127, 0, 0, 1}, 0};
	if(role=='i' || role=='t') {
		// I have N getter ports!
		for(i=0; i<N; i++) {
			addr.port = min_pid+i;
			connector_add_net_port(c, &getters[i], addr, Polarity_Getter, EndpointPolarity_Passive);
		}
	}
	if(role=='h' || role=='i') {
		// I have N putter ports!
		for(i=0; i<N; i++) {
			addr.port = min_pid+i+N;
			connector_add_net_port(c, &putters[i], addr, Polarity_Putter, EndpointPolarity_Active);
		}
	}
	printf("Added all ports!\n");
	if(role=='i') {
		// Inner has a forwarder component to forward messages
		for(i=0; i<N; i++) {	
			connector_add_component(c, "forward", 7, (PortId[]){putters[i], getters[i]}, 2);
		}
	}
	connector_connect(c, -1);
	
	clock_t begin = clock();
	char msg[] = "Hello, world!";
	for (i=0; i<10000; i++) {
		if(role=='h' || role=='s') {
			// singleton and head send N messages
			for(i=0; i<N; i++) { 
				connector_put_bytes(c, putters[i], msg, sizeof(msg)-1);
			}
		}
		if(role=='t' || role=='s') {
			// singleton and tail recv N messages
			for(i=0; i<N; i++) { 
				connector_get(c, getters[i]);
			}
		}
		// inner doesn't send nor receive
		connector_sync(c, -1);
	}
	clock_t end = clock();
	double time_spent = (double)(end - begin) / CLOCKS_PER_SEC;
	printf("Time taken: %f\n", time_spent);
	return 0;
}