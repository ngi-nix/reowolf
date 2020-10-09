#include <time.h>
#include "../../reowolf.h"
#include "../utility.c"
FfiSocketAddr addr_new(const uint8_t ipv4[4], uint16_t port) {
    FfiSocketAddr x;
    x.port = port;
    memcpy(x.ipv4, ipv4, sizeof(uint8_t)*4);
    return x;
}
int main(int argc, char** argv) {
    int i, rounds;
    char optimized = argv[1][0];
    char sender = argv[2][0];
    rounds = atoi(argv[3]);
    uint8_t ipv4[4] = { atoi(argv[4]), atoi(argv[5]), atoi(argv[6]), atoi(argv[7]) };
    size_t msg_len = atoi(argv[8]);

    printf("optimized %c, sender %c, rounds %d, addr %d.%d.%d.%d, msg_len %d\n",
        optimized, sender, rounds, ipv4[0], ipv4[1], ipv4[2], ipv4[3], msg_len);

    unsigned char pdl[] = "\
    primitive filter(in i, out o) {\
        while(true) synchronous() {\
            msg m = get(i);\
            if(m[0] == 0) put(o, m);\
        }\
    }\
    ";
    Arc_ProtocolDescription * pd = protocol_description_parse(pdl, sizeof(pdl)-1);
    printf("Error str `%s`\n", reowolf_error_peek(NULL));
    Connector * c = connector_new_with_id(pd, sender=='y'?1:0);
    PortId ports[3]; // orientation: 0->1->2 (subsets may be initialized) sender puts on 0. !sender gets on 2. 
    char ident[] = "filter";
    FfiSocketAddr addr = addr_new(ipv4, 7000);
    if(sender=='y') {
        Polarity p = Polarity_Putter;
        EndpointPolarity ep = EndpointPolarity_Active;
        if(optimized=='y') {
            // 3 ports: (native)0-->1(filter)2-->(NETWORK)
            connector_add_port_pair(c, &ports[0], &ports[1]);
            connector_add_net_port(c, &ports[2], addr, p, ep);
            printf("Error str `%s`\n", reowolf_error_peek(NULL));
            connector_add_component(c, ident, sizeof(ident)-1, ports+1, 2);
            printf("Error str `%s`\n", reowolf_error_peek(NULL));
        } else {
            // 1 port
            connector_add_net_port(c, &ports[0], addr, p, ep);
            printf("Error str `%s`\n", reowolf_error_peek(NULL));
        }
    } else {
        Polarity p = Polarity_Getter;
        EndpointPolarity ep = EndpointPolarity_Passive;
        if(optimized=='y') {
            // 1 port
            connector_add_net_port(c, &ports[2], addr, p, ep);
            printf("Error str `%s`\n", reowolf_error_peek(NULL));
        } else {
            // 3 ports: (NETWORK)-->0(filter)1-->2(native)
            connector_add_net_port(c, &ports[0], addr, p, ep);
            printf("Error str `%s`\n", reowolf_error_peek(NULL));
            connector_add_port_pair(c, &ports[1], &ports[2]);
            connector_add_component(c, ident, sizeof(ident)-1, ports, 2);
            printf("Error str `%s`\n", reowolf_error_peek(NULL));
        }
    }
    connector_connect(c, -1);
    printf("Error str `%s`\n", reowolf_error_peek(NULL));

    char * msg = malloc(msg_len);
    memset(msg, 42, msg_len);
    
    clock_t begin = clock();
    for (i=0; i<rounds; i++) {
        if(sender=='y') {
            msg[0] = (char) i%2;
            connector_put_bytes(c, ports[0], msg, msg_len);
            // always put
        } else {
            // no-get option
            connector_next_batch(c);
            // get option
            connector_get(c, ports[2]);
        }
        connector_sync(c, -1);
    }
    clock_t end = clock();
    double time_spent = (double)(end - begin) / CLOCKS_PER_SEC;
    printf("Time Spent: %f\n", time_spent);

    free(msg);
    return 0;
}