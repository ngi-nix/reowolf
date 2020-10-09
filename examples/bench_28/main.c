#include <time.h>
#include "../../reowolf.h"
#include "../utility.c"
int main(int argc, char** argv) {
    int i, rounds;
    char optimized = argv[1][0];
    rounds = atoi(argv[2]);
    printf("optimized %c, rounds %d\n", optimized, rounds);

    unsigned char pdl[] = "";
    Arc_ProtocolDescription * pd = protocol_description_parse(pdl, sizeof(pdl)-1);
    printf("Error str `%s`\n", reowolf_error_peek(NULL));
    Connector * c = connector_new_with_id(pd, 0);
    PortId ports[6];
    for(i=0; i<3; i++) {
        connector_add_port_pair(c, &ports[i*2+0], &ports[i*2+1]);
    }
    connector_add_component(c, "sync", 4, ports+1, 2);
    if(optimized=='y') {
        connector_add_component(c, "forward", 7, ports+3, 2);
    } else {
        connector_add_component(c, "sync",    4, ports+3, 2);
    }
    connector_connect(c, -1);
    printf("Error str `%s`\n", reowolf_error_peek(NULL));

    size_t msg_len = 1000;
    char * msg = malloc(msg_len);
    memset(msg, 42, msg_len);
    
    clock_t begin = clock();
    for (i=0; i<rounds; i++) {
        connector_put_bytes(c, ports[0], msg, msg_len);
        connector_get(c, ports[5]);
        connector_sync(c, -1);
    }
    clock_t end = clock();
    double time_spent = (double)(end - begin) / CLOCKS_PER_SEC;
    printf("Time Spent: %f\n", time_spent);

    free(msg);
    return 0;
}