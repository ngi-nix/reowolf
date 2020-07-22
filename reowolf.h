/* CBindgen generated */

#ifndef REOWOLF_HEADER_DEFINED
#define REOWOLF_HEADER_DEFINED

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

#define BAD_FD -5

#define CC_MAP_LOCK_POISONED -3

#define CLOSE_FAIL -4

#define CONNECT_FAILED -6

#define ERR_OK 0

#define ERR_REOWOLF -1

#define WOULD_BLOCK -7

#define WRONG_STATE -2

typedef enum {
  EndpointPolarity_Active,
  EndpointPolarity_Passive,
} EndpointPolarity;

typedef enum {
  Polarity_Putter,
  Polarity_Getter,
} Polarity;

typedef struct Arc_ProtocolDescription Arc_ProtocolDescription;

typedef struct Connector Connector;

typedef uint32_t ConnectorId;

typedef uint32_t U32Suffix;

typedef struct {
  ConnectorId connector_id;
  U32Suffix u32_suffix;
} Id;

typedef Id PortId;

typedef struct {
  uint8_t ipv4[4];
  uint16_t port;
} FfiSocketAddr;

/**
 * Given
 * - an initialized connector in setup or connecting state,
 * - a string slice for the component's identifier in the connector's configured protocol description,
 * - a set of ports (represented as a slice; duplicates are ignored) in the native component's interface,
 * the connector creates a new (internal) protocol component C, such that the set of native ports are moved to C.
 * Usable in {setup, communication} states.
 */
int connector_add_component(Connector *connector,
                            const uint8_t *ident_ptr,
                            uintptr_t ident_len,
                            const PortId *ports_ptr,
                            uintptr_t ports_len);

/**
 * Given
 * - an initialized connector in setup or connecting state,
 * - a utf-8 encoded socket address,
 * - the logical polarity of P,
 * - the "physical" polarity in {Active, Passive} of the endpoint through which P's peer will be discovered,
 * returns P, a port newly added to the native interface.
 */
int connector_add_net_port(Connector *connector,
                           PortId *port,
                           FfiSocketAddr addr,
                           Polarity port_polarity,
                           EndpointPolarity endpoint_polarity);

/**
 * Given an initialized connector in setup or connecting state,
 * - Creates a new directed port pair with logical channel putter->getter,
 * - adds the ports to the native component's interface,
 * - and returns them using the given out pointers.
 * Usable in {setup, communication} states.
 */
void connector_add_port_pair(Connector *connector, PortId *out_putter, PortId *out_getter);

/**
 * Given
 * - an initialized connector in setup or connecting state,
 * - a utf-8 encoded BIND socket addresses (i.e., "local"),
 * - a utf-8 encoded CONNECT socket addresses (i.e., "peer"),
 * returns [P, G] via out pointers [putter, getter],
 * - where P is a Putter port that sends messages into the socket
 * - where G is a Getter port that recvs messages from the socket
 */
int connector_add_udp_port_pair(Connector *connector,
                                PortId *putter,
                                PortId *getter,
                                FfiSocketAddr local_addr,
                                FfiSocketAddr peer_addr);

/**
 * Connects this connector to the distributed system of connectors reachable through endpoints,
 * Usable in setup state, and changes the state to communication.
 */
int connector_connect(Connector *connector, int64_t timeout_millis);

/**
 * Destroys the given a pointer to the connector on the heap, freeing its resources.
 * Usable in {setup, communication} states.
 */
void connector_destroy(Connector *connector);

int connector_get(Connector *connector, PortId port);

const uint8_t *connector_gotten_bytes(Connector *connector, PortId port, uintptr_t *out_len);

/**
 * Initializes `out` with a new connector using the given protocol description as its configuration.
 * The connector uses the given (internal) connector ID.
 */
Connector *connector_new(const Arc_ProtocolDescription *pd);

Connector *connector_new_logging(const Arc_ProtocolDescription *pd,
                                 const uint8_t *path_ptr,
                                 uintptr_t path_len);

intptr_t connector_next_batch(Connector *connector);

void connector_print_debug(Connector *connector);

/**
 * Convenience function combining the functionalities of
 * "payload_new" with "connector_put_payload".
 */
int connector_put_bytes(Connector *connector,
                        PortId port,
                        const uint8_t *bytes_ptr,
                        uintptr_t bytes_len);

intptr_t connector_sync(Connector *connector, int64_t timeout_millis);

/**
 * Given an initialized protocol description, initializes `out` with a clone which can be independently created or destroyed.
 */
Arc_ProtocolDescription *protocol_description_clone(const Arc_ProtocolDescription *pd);

/**
 * Destroys the given initialized protocol description and frees its resources.
 */
void protocol_description_destroy(Arc_ProtocolDescription *pd);

/**
 * Parses the utf8-encoded string slice to initialize a new protocol description object.
 * - On success, initializes `out` and returns 0
 * - On failure, stores an error string (see `reowolf_error_peek`) and returns -1
 */
Arc_ProtocolDescription *protocol_description_parse(const uint8_t *pdl, uintptr_t pdl_len);

/**
 * Returns length (via out pointer) and pointer (via return value) of the last Reowolf error.
 * - pointer is NULL iff there was no last error
 * - data at pointer is null-delimited
 * - len does NOT include the length of the null-delimiter
 * If len is NULL, it will not written to.
 */
const uint8_t *reowolf_error_peek(uintptr_t *len);

#endif /* REOWOLF_HEADER_DEFINED */
