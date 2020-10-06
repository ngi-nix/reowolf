#include <time.h>
#include "../../reowolf.h"
#include "../utility.c"
int main(int argc, char** argv) {
	// same as bench 15 but connecting to 87.210.104.102 and getting at 0.0.0.0
	// also, doing 10k reps (down from 100k) to save time
	int i, cid;
	cid = atoi(argv[1]);
	printf("cid %d\n", cid);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));

	unsigned char pdl[] = "";
	Arc_ProtocolDescription * pd = protocol_description_parse(pdl, sizeof(pdl)-1);
	Connector * c = connector_new_with_id(pd, cid);

	bool seen_delim = false;
	for(i=2; i<argc; i++) {
		EndpointPolarity ep;
		Polarity p;
		FfiSocketAddr addr;
		if(argv[i][0] == '.') {
			seen_delim = true;
			continue;
		} else if(seen_delim) {
			addr = (FfiSocketAddr) {{0, 0, 0, 0}, atoi(argv[i])};
			printf("getter");
			p = Polarity_Getter;
			ep = EndpointPolarity_Passive;
		} else {
			addr = (FfiSocketAddr) {{87, 210, 104, 102}, atoi(argv[i])};
			printf("putter");
			p = Polarity_Putter;
			ep = EndpointPolarity_Active;
		}
		printf("@%d\n", addr.port);
		connector_add_net_port(c, NULL, addr, p, ep);
	}
	printf("Added all ports!\n");
	connector_connect(c, -1);
	printf("Connect OK!\n");
	
	clock_t begin = clock();
	for (i=0; i<150; i++) {
		connector_sync(c, -1);
	}
	clock_t end = clock();
	double time_spent = (double)(end - begin) / CLOCKS_PER_SEC;
	printf("Time taken: %f\n", time_spent);
	return 0;
}