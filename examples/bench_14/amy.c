#include <time.h>
#include "../../reowolf.h"
#include "../utility.c"
int main(int argc, char** argv) {
	int i, msglen;
	msglen = atoi(argv[1]);
	printf("msglen %d\n", msglen);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	char * msg = malloc(msglen);
	memset(msg, msglen, 42);

	unsigned char pdl[] = "";
	Arc_ProtocolDescription * pd = protocol_description_parse(pdl, sizeof(pdl)-1);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	char logpath[] = "./bench_14_amy.txt";
	Connector * c = connector_new_logging_with_id(pd, logpath, sizeof(logpath)-1, 0);
	
	PortId putter, getter;
	connector_add_net_port(
		c,
		&putter, 
		(FfiSocketAddr) {{127, 0, 0, 1}, 7000},
		Polarity_Putter,
		EndpointPolarity_Active);
	connector_add_net_port(
		c,
		&getter, 
		(FfiSocketAddr) {{127, 0, 0, 1}, 7001},
		Polarity_Getter,
		EndpointPolarity_Passive);
	connector_connect(c, -1);
	
	clock_t begin = clock();
	for (i=0; i<10000; i++) {
		connector_put_bytes(c, putter, msg, msglen);
		connector_get(c, getter);
		connector_sync(c, -1);
	}
	clock_t end = clock();
	double time_spent = (double)(end - begin) / CLOCKS_PER_SEC;
	printf("Time taken: %f\n", time_spent);

	free(msg);
	return 0;
}