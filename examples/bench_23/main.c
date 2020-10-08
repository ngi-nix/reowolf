#include <time.h>
#include "../../reowolf.h"
#include "../utility.c"
int main(int argc, char** argv) {
	int i;

	// unsigned char pdl[] = "\
	// primitive xrouter(in a, out b, out c) {\
 //        while(true) synchronous {\
 //            if(fires(a)) {\
 //                if(fires(b)) put(b, get(a));\
 //                else         put(c, get(a));\
 //            }\
 //        }\
 //    }"
 //    ;
	unsigned char pdl[] = "\
	primitive lossy(in a, out b) {\
        while(true) synchronous {\
            if(fires(a)) {\
                msg m = get(a);\
                if(fires(b)) put(b, m);\
            }\
        }\
    }\
    primitive sync_drain(in a, in b) {\
        while(true) synchronous {\
            if(fires(a)) {\
                get(a);\
                get(b);\
            }\
        }\
    }\
    composite xrouter(in a, out b, out c) {\
        channel d -> e;\
        channel f -> g;\
        channel h -> i;\
        channel j -> k;\
        channel l -> m;\
        channel n -> o;\
        channel p -> q;\
        channel r -> s;\
        channel t -> u;\
        new replicator(a, d, f);\
        new replicator(g, t, h);\
        new lossy(e, l);\
        new lossy(i, j);\
        new replicator(m, b, p);\
        new replicator(k, n, c);\
        new merger(q, o, r);\
        new sync_drain(u, s);\
    }"
    ;
	Arc_ProtocolDescription * pd = protocol_description_parse(pdl, sizeof(pdl)-1);
	Connector * c = connector_new_with_id(pd, 0);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));

	PortId ports[6];
	for(i=0; i<3; i++) {
		connector_add_port_pair(c, &ports[2*i], &ports[2*i+1]);
	}
	// [native~~~~~~~~~~]
	//  0  1  2  3  4  5
	//  |  ^  |  ^  |  ^  
	//  `--`  `--`  `--`  
	char ident[] = "xrouter";
	connector_add_component(
		c,
		ident,
		sizeof(ident)-1,
		(PortId[]) { ports[1], ports[2], ports[4] },
		3);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));

	// [native~~~~~~~~~~]
	//  0        3     5
	//  V        ^     ^  
	//  1        2     4  
	// [xrouter~~~~~~~~~]
	connector_connect(c, -1);
	printf("Connect OK!\n");
	
	int msg_len = 1000;
	char * msg = malloc(msg_len);
	memset(msg, 42, msg_len);

	{
		clock_t begin = clock();
		for (i=0; i<100000; i++) {
			connector_put_bytes(c, ports[0], msg, msg_len);
			connector_get(c, ports[3]);
			connector_sync(c, -1);
		}
		clock_t end = clock();
		double time_spent = (double)(end - begin) / CLOCKS_PER_SEC;
		printf("First: %f\n", time_spent);
	}
	{
		clock_t begin = clock();
		for (i=0; i<100000; i++) {
			connector_put_bytes(c, ports[0], msg, msg_len);
			connector_get(c, ports[5]);
			connector_sync(c, -1);
		}
		clock_t end = clock();
		double time_spent = (double)(end - begin) / CLOCKS_PER_SEC;
		printf("Second: %f\n", time_spent);
	}
	{
		clock_t begin = clock();
		for (i=0; i<100000; i++) {
			connector_put_bytes(c, ports[0], msg, msg_len);
			connector_get(c, ports[3 + (i%2)*2]);
			connector_sync(c, -1);
		}
		clock_t end = clock();
		double time_spent = (double)(end - begin) / CLOCKS_PER_SEC;
		printf("Alternating: %f\n", time_spent);
	}
	free(msg);
	return 0;
}