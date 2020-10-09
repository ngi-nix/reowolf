#include <time.h>
#include "../../reowolf.h"
#include "../utility.c"
int main(int argc, char** argv) {
    int i, rounds;
    char optimized = argv[1][0];
    rounds = atoi(argv[2]);
    printf("optimized %c, rounds %d\n", optimized, rounds);

    unsigned char pdl[] = "\
    primitive xrouter(in a, out b, out c) {\
        while(true) synchronous {\
            if(fires(a)) {\
                if(fires(b)) put(b, get(a));\
                else         put(c, get(a));\
            }\
        }\
    }\
    ";
    Arc_ProtocolDescription * pd = protocol_description_parse(pdl, sizeof(pdl)-1);
    printf("Error str `%s`\n", reowolf_error_peek(NULL));
    Connector * c = connector_new_with_id(pd, 0);
    PortId ports[8];
    if(optimized=='y') {
        connector_add_port_pair(c, &ports[0], &ports[1]);
        connector_add_port_pair(c, &ports[2], &ports[7]); // 3,4,5,6 uninitialized
        connector_add_component(c, "sync", 4, ports+1, 2);
        printf("Error str `%s`\n", reowolf_error_peek(NULL));
    } else {
        for(i=0; i<4; i++) {
            connector_add_port_pair(c, &ports[i*2+0], &ports[i*2+1]);
        }
        connector_add_component(c, "xrouter", 7, (PortId[]) {ports[1],ports[2],ports[4]}, 3);
        printf("Error str `%s`\n", reowolf_error_peek(NULL));
        connector_add_component(c, "merger" , 6, (PortId[]) {ports[3],ports[5],ports[6]}, 3);
        printf("Error str `%s`\n", reowolf_error_peek(NULL));
    }
    connector_connect(c, -1);
    printf("Error str `%s`\n", reowolf_error_peek(NULL));

    size_t msg_len = 1000;
    char * msg = malloc(msg_len);
    memset(msg, 42, msg_len);
    
    clock_t begin = clock();
    for (i=0; i<rounds; i++) {
        connector_put_bytes(c, ports[0], msg, msg_len);
        connector_get(c, ports[7]);
        connector_sync(c, -1);
    }
    clock_t end = clock();
    double time_spent = (double)(end - begin) / CLOCKS_PER_SEC;
    printf("Time Spent: %f\n", time_spent);

    free(msg);
    return 0;
}