#include <stdio.h>
#include <string.h>
#include "../../reowolf.h"
#include "../utility.c"

int main(int argc, char** argv) {
    if(argc != 3) {
        printf("Expected arg[1] and arg[2] for use as addr str\n");
        exit(1);
    }            
    char * pdl_ptr = buffer_pdl("eg_protocols.pdl");
    size_t pdl_len = strlen(pdl_ptr);
    Arc_ProtocolDescription * pd = protocol_description_parse(pdl_ptr, pdl_len);
    char logpath[] = "./a_amy_log.txt";
    Connector * c = connector_new_logging(pd, logpath, sizeof(logpath)-1);
    printf("Error str `%s`\n", reowolf_error_peek(NULL));
    
    PortId ports[6]; 
    connector_add_port_pair(c, &ports[0], &ports[1]);
    printf("Error str `%s`\n", reowolf_error_peek(NULL));
    connector_add_net_port(c, &ports[2], argv[1], strlen(argv[1]), Getter, Passive);
    printf("Error str `%s`\n", reowolf_error_peek(NULL));
    connector_add_net_port(c, &ports[3], argv[2], strlen(argv[2]), Putter, Active);
    printf("Error str `%s`\n", reowolf_error_peek(NULL));
    connector_add_port_pair(c, &ports[4], &ports[5]);
    printf("Error str `%s`\n", reowolf_error_peek(NULL));
    // native {0,1,2,3,4,5}
    
    connector_add_component(c, "together", 8, &ports[1], 4);
    printf("Error str `%s`\n", reowolf_error_peek(NULL));
    // native {0,5} together {1,2,3,4}
    
    connector_connect(c, 4000);
    printf("Error str `%s`\n", reowolf_error_peek(NULL));
	
	connector_put_bytes(c, ports[0], "hi", 2);
    printf("Error str `%s`\n", reowolf_error_peek(NULL));
	connector_get(c, ports[5]);
    printf("Error str `%s`\n", reowolf_error_peek(NULL));
	
    connector_sync(c, 1000);
    printf("Error str `%s`\n", reowolf_error_peek(NULL));
	
	size_t msg_len;
	const char * msg_ptr = connector_gotten_bytes(c, ports[5], &msg_len);
    printf("Error str `%s`\n", reowolf_error_peek(NULL));
	printf("Got msg `%.*s`\n", msg_len, msg_ptr);
    
    protocol_description_destroy(pd);
    connector_destroy(c);
    return 0;
}