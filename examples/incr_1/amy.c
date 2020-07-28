/* This example demonstrates:
- how protocol description structures are created and destroyed
- how connectors are created and destroyed
- there are procedures allowing for debugging the states of connectors
*/
#include <stdio.h>
#include <string.h>
#include "../../reowolf.h"
#include "../utility.c"

int main(int argc, char** argv) {
	Arc_ProtocolDescription * pd = protocol_description_parse("", 0);
	Connector * c = connector_new(pd);
	connector_print_debug(c);
	
	protocol_description_destroy(pd);
	connector_print_debug(c);
	connector_destroy(c);
	return 0;
}