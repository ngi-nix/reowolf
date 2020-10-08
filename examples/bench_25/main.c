#include <time.h>
#include "../../reowolf.h"
#include "../utility.c"
int main(int argc, char** argv) {
    int i, j, syncs;
    syncs = atoi(argv[1]);
    printf("syncs %d\n", syncs);

    unsigned char pdl[] = "";
    Arc_ProtocolDescription * pd = protocol_description_parse(pdl, sizeof(pdl)-1);
    printf("Error str `%s`\n", reowolf_error_peek(NULL));
    char logpath[] = "./bench_11.txt";
    Connector * c = connector_new_logging(pd, logpath, sizeof(logpath)-1);

    PortId native_putter, native_getter;
    connector_add_port_pair(c, &native_putter, &native_getter);
    for (i=0; i<syncs; i++) {
        // create a forward to tail of chain
        PortId putter, getter;
        connector_add_port_pair(c, &putter, &getter);
        // native ports: {native_putter, native_getter, putter, getter}
        // thread a forward component onto native_tail
        char ident[] = "sync";
        connector_add_component(c, ident, sizeof(ident)-1, (PortId[]){native_getter, putter}, 2);
        // native ports: {native_putter, getter}
        printf("Error str `%s`\n", reowolf_error_peek(NULL));
        native_getter = getter;
    }
    // add "recv_zero" on end of chain
    connector_connect(c, -1);
    printf("Error str `%s`\n", reowolf_error_peek(NULL));

    size_t msg_len = 1000;
    char * msg = malloc(msg_len);
    memset(msg, 42, msg_len);
    
    {
        clock_t begin = clock();
        for (i=0; i<1000000; i++) {
            connector_put_bytes(c, native_putter, msg, msg_len);
            connector_get(c, native_getter);
            connector_sync(c, -1);  
        }
        clock_t end = clock();
        double time_spent = (double)(end - begin) / CLOCKS_PER_SEC;
        printf("Sending: %f\n", time_spent);
    }
    {
        clock_t begin = clock();
        for (i=0; i<1000000; i++) {
            connector_sync(c, -1);  
        }
        clock_t end = clock();
        double time_spent = (double)(end - begin) / CLOCKS_PER_SEC;
        printf("Not Sending: %f\n", time_spent);
    }

    free(msg);
    return 0;
}