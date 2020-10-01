#include <time.h>
#include "../../reowolf.h"
#include "../utility.c"
int main(int argc, char** argv) {
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
		if(argv[i][0] == '.') {
			seen_delim = true;
			continue;
		} else if(seen_delim) {
			printf("putter");
			p = Polarity_Getter;
			ep = EndpointPolarity_Passive;
		} else {
			printf("getter");
			p = Polarity_Putter;
			ep = EndpointPolarity_Active;
		}
		FfiSocketAddr addr = {{127, 0, 0, 1}, atoi(argv[i])};
		printf("@%d\n", addr.port);
		connector_add_net_port(c, NULL, addr, p, ep);
	}
	printf("Added all ports!\n");
	connector_connect(c, -1);
	
	clock_t begin = clock();
	for (i=0; i<10000; i++) {
		connector_sync(c, -1);
	}
	clock_t end = clock();
	double time_spent = (double)(end - begin) / CLOCKS_PER_SEC;
	printf("Time taken: %f\n", time_spent);
	return 0;
}