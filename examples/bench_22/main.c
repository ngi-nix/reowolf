#include <time.h>
#include "../../reowolf.h"
#include "../utility.c"


int main(int argc, char** argv) {
	// same as bench 21 but with parametric message length
	int i, j, cid, msg_loops, par_msgs, msg_len;
	char is_leader;
	is_leader = argv[1][0];
	msg_loops = atoi(argv[2]);
	par_msgs = atoi(argv[3]);
	msg_len = atoi(argv[8]);

	// argv[4..8] encodes peer IP
	printf("is_leader %c, msg_loops %d, par_msgs %d, msg_len %d\n", is_leader, msg_loops, par_msgs, msg_len);
	cid = is_leader=='y'; // cid := { leader:1, follower:0 }

	unsigned char pdl[] = "";
	Arc_ProtocolDescription * pd = protocol_description_parse(pdl, sizeof(pdl)-1);
	Connector * c = connector_new_with_id(pd, cid);
	PortId native_ports[par_msgs];
	FfiSocketAddr peer_addr = {
		{
			atoi(argv[4]),
			atoi(argv[5]),
			atoi(argv[6]),
			atoi(argv[7])
		}, 0/*dummy value*/};
	int port = 7000;
	
	// for each parallel message 
	for(i=0; i<par_msgs; i++) {
		if(is_leader == 'y') {
			peer_addr.port = port++;
			connector_add_net_port(
				c, &native_ports[i], peer_addr, Polarity_Putter, EndpointPolarity_Active);
			printf("Error str `%s`\n", reowolf_error_peek(NULL));
		}

		for(j=0; j<msg_loops; j++) {
			PortId loop_getter, loop_putter;

			// create {putter, getter} port pair
			connector_add_net_port(
				c, &loop_getter,
				(FfiSocketAddr) {{0,0,0,0}, port++},
				Polarity_Getter, EndpointPolarity_Passive);
			printf("Error str `%s`\n", reowolf_error_peek(NULL));

			peer_addr.port = port++;
			connector_add_net_port(
				c, &loop_putter, peer_addr, Polarity_Putter, EndpointPolarity_Active);
			printf("Error str `%s`\n", reowolf_error_peek(NULL));

			connector_add_component(c, "forward", 7,
				(PortId[]){loop_getter, loop_putter}, 2);
			printf("Error str `%s`\n", reowolf_error_peek(NULL));
		}

		if(is_leader != 'y') {
			connector_add_net_port(
				c, &native_ports[i],
				(FfiSocketAddr) {{0,0,0,0}, port++},
				Polarity_Getter, EndpointPolarity_Passive);
			printf("Error str `%s`\n", reowolf_error_peek(NULL));
		}
	}
	printf("Added all ports!\n");
	connector_connect(c, -1);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	printf("Connect OK!\n");

	char * msg = malloc(msg_len);
	memset(msg, 42, msg_len);
	
	clock_t begin = clock();
	for(i=0; i<100000; i++) {
		if(is_leader == 'y') {
			for(j=0; j<par_msgs; j++) {
				connector_put_bytes(c, native_ports[j], msg, msg_len);
			}
		} else {
			for(j=0; j<par_msgs; j++) {
				connector_get(c, native_ports[j]);
			}
		}
		connector_sync(c, -1);
	}
	
	clock_t end = clock();
	double time_spent = (double)(end - begin) / CLOCKS_PER_SEC;
	printf("Time taken: %f\n", time_spent);
	free(msg);
	return 0;
}