#include <time.h>
#include "../../reowolf.h"
#include "../utility.c"
int main(int argc, char** argv) {
	int i, j;

	unsigned char pdl[] = "\
	primitive fifo1_init(msg m, in a, out b) {\
        while(true) synchronous {\
            if(m != null && fires(b)) {\
                put(b, m);\
                m = null;\
            } else if (m == null && fires(a)) {\
                m = get(a);\
            }\
        }\
    }\
    composite fifo1_full(in a, out b) {\
        new fifo1_init(create(0), a, b);\
    }\
    composite fifo1(in a, out b) {\
        new fifo1_init(null, a, b);\
    }\
    composite sequencer3(out a, out b, out c) {\
        channel d -> e;\
        channel f -> g;\
        channel h -> i;\
        channel j -> k;\
        channel l -> m;\
        channel n -> o;\
        new fifo1_full(o, d);\
        new replicator(e, f, a);\
        new fifo1(g, h);\
        new replicator(i, j, b);\
        new fifo1(k, l);\
        new replicator(m, n, c);\
    }"
    ;
	// unsigned char pdl[] = "\
	// primitive sequencer3(out a, out b, out c) {\
 //        int i = 0;\
 //        while(true) synchronous {\
 //            out to = a;\
 //            if     (i==1) to = b;\
 //            else if(i==2) to = c;\
 //            if(fires(to)) {\
 //                put(to, create(0));\
 //                i = (i + 1)%3;\
 //            }\
 //        }\
 //    }"
    ;
	Arc_ProtocolDescription * pd = protocol_description_parse(pdl, sizeof(pdl)-1);
	Connector * c = connector_new_with_id(pd, 0);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));

	PortId putters[3], getters[3];
	for(i=0; i<3; i++) {
		connector_add_port_pair(c, &putters[i], &getters[i]);
	}
	char ident[] = "sequencer3";
	connector_add_component(c, ident, sizeof(ident)-1, putters, 3);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	connector_connect(c, -1);
	printf("Connect OK!\n");

	clock_t begin = clock();
	for (i=0; i<1000000/3; i++) {
		for (j=0; j<3; j++) {
			connector_get(c, getters[j]);
			connector_sync(c, -1);
		}
	}
	clock_t end = clock();
	double time_spent = (double)(end - begin) / CLOCKS_PER_SEC;
	printf("Time taken: %f\n", time_spent);
	return 0;
}