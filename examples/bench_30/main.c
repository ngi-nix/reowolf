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

    unsigned char pdl[] = "";
    Arc_ProtocolDescription * pd = protocol_description_parse(pdl, sizeof(pdl)-1);
    printf("Error str `%s`\n", reowolf_error_peek(NULL));
    Connector * c = connector_new_with_id(pd, sender=='y'?1:0);

    PortId ports[5]; // sender always puts 0, receiver always gets 3, 4
    char ident[] = "replicator";
    FfiSocketAddr addrs[2] = {
        addr_new(ipv4, 7000),
        addr_new(ipv4, 7001)
    };
    if(sender=='y') {
        Polarity p = Polarity_Putter;
        EndpointPolarity ep = EndpointPolarity_Active;
        if(optimized=='y') {
            // 1 port: (native)0-->(NETWORK)
            connector_add_net_port(c, &ports[0], addrs[0], p, ep);
            printf("Error str `%s`\n", reowolf_error_peek(NULL));
        } else {
            // 4 ports: (native)0-->1(replicator)2-->(NETWORK)
            //                                   3-->(NETWORK)
            connector_add_port_pair(c, &ports[0], &ports[1]);
            connector_add_net_port(c, &ports[2], addrs[0], p, ep);
            printf("Error str `%s`\n", reowolf_error_peek(NULL));
            connector_add_net_port(c, &ports[3], addrs[1], p, ep);
            printf("Error str `%s`\n", reowolf_error_peek(NULL));
            connector_add_component(c, ident, sizeof(ident)-1, ports+1, 3);
            printf("Error str `%s`\n", reowolf_error_peek(NULL));
        }
    } else {
        Polarity p = Polarity_Getter;
        EndpointPolarity ep = EndpointPolarity_Passive;
        if(optimized=='y') {
            // 5 ports: (NETWORK)-->0(replicator)1-->3(native)
            //                                   2-->4
            connector_add_net_port(c, &ports[0], addrs[0], p, ep);
            printf("Error str `%s`\n", reowolf_error_peek(NULL));
            connector_add_port_pair(c, &ports[1], &ports[3]);
            connector_add_port_pair(c, &ports[2], &ports[4]);
            connector_add_component(c, ident, sizeof(ident)-1, ports, 3);
            printf("Error str `%s`\n", reowolf_error_peek(NULL));
        } else {
            // 2 ports: (NETWORK)-->3(native)
            //                   -->4
            connector_add_net_port(c, &ports[3], addrs[0], p, ep);
            printf("Error str `%s`\n", reowolf_error_peek(NULL));
            connector_add_net_port(c, &ports[4], addrs[1], p, ep);
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
            connector_put_bytes(c, ports[0], msg, msg_len);
        } else {
            connector_get(c, ports[3]);
            connector_get(c, ports[4]);
        }
        connector_sync(c, -1);
    }
    clock_t end = clock();
    double time_spent = (double)(end - begin) / CLOCKS_PER_SEC;
    printf("Time Spent: %f\n", time_spent);

    free(msg);
    return 0;
}