#include <time.h>
#include "../../reowolf.h"
#include "../utility.c"
FfiSocketAddr addr_new(const uint8_t ipv4[4], uint16_t port) {
    FfiSocketAddr x;
    x.port = port;
    int i;
    for(i=0; i<4; i++) {
        x.ipv4[i] = ipv4[i];
    }
    return x;
}
int main(int argc, char** argv) {
    int i, rounds;
    char leader;
    uint8_t ipv4[4] = { atoi(argv[1]), atoi(argv[2]), atoi(argv[3]), atoi(argv[4]) };
    leader = argv[5][0];
    rounds = atoi(argv[6]);
    printf("leader %c, rounds %d, peer at %d.%d.%d.%d\n",
        leader, rounds, ipv4[0], ipv4[1], ipv4[2], ipv4[3]);

    unsigned char pdl[] = "";
    Arc_ProtocolDescription * pd = protocol_description_parse(pdl, sizeof(pdl)-1);
    printf("Error str `%s`\n", reowolf_error_peek(NULL));
    Connector * c = connector_new_with_id(pd, leader=='y'?1:0);
    PortId ports[2];
    if(leader=='y') {
        EndpointPolarity ep = EndpointPolarity_Active;
        connector_add_net_port(c, &ports[0], addr_new(ipv4, 7005), Polarity_Putter, ep);
        connector_add_net_port(c, &ports[1], addr_new(ipv4, 7006), Polarity_Getter, ep);
    } else {
        EndpointPolarity ep = EndpointPolarity_Passive;
        connector_add_net_port(c, &ports[0], addr_new(ipv4, 7005), Polarity_Getter, ep);
        connector_add_net_port(c, &ports[1], addr_new(ipv4, 7006), Polarity_Putter, ep);
        char ident[] = "sync";
        connector_add_component(c, ident, sizeof(ident)-1, ports, 2);
        printf("Error str `%s`\n", reowolf_error_peek(NULL));
    }
    connector_connect(c, -1);
    printf("Error str `%s`\n", reowolf_error_peek(NULL));

    size_t msg_len = 1000;
    char * msg = malloc(msg_len);
    memset(msg, 42, msg_len);
    
    clock_t begin = clock();
    for (i=0; i<rounds; i++) {
        if(leader=='y') {
            connector_put_bytes(c, ports[0], msg, msg_len);
            connector_get(c, ports[1]);
        }
        connector_sync(c, -1);  
    }
    clock_t end = clock();
    double time_spent = (double)(end - begin) / CLOCKS_PER_SEC;
    printf("Time Spent: %f\n", time_spent);

    free(msg);
    return 0;
}