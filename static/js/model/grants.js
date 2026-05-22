/**
 * @import {Grant, ResourceTypeEnum, SharedWithMeResponse} from '../core/types.js'
 */

const grants = {
    /** @type {Record<String, Record<String, Grant[]>>} */
    outgoingGrants: {},

    /** @type {Record<String, Record<String, Grant[]>>} */
    incomingGrants: {},

    async fetchOutgoingGrants() {
        const response = await fetch('/api/grants/outgoing');

        if (!response.ok) {
            console.log(`error ${response.status} while fetching /api/grants/outgoing:`, await response.json());
            return;
        }

        /** @type {Grant[]} */
        const outgoingGrants = await response.json();

        console.log(outgoingGrants);

        // store grants by type, then by id
        outgoingGrants.forEach((grant) => {
            this.outgoingGrants[grant.resource.type] ??= {};
            this.outgoingGrants[grant.resource.type][grant.resource.id] ??= [];
            this.outgoingGrants[grant.resource.type][grant.resource.id].push(grant);
        });

        console.log(`outgoing grants: `, this.outgoingGrants);
    },

    /**
     * get grant for a resource
     * @param {ResourceTypeEnum} resourceType
     * @param {String} id
     * @returns {Grant[] | null}
     */
    getOutgoingGrantsFor(resourceType, id) {
        try {
            return this.outgoingGrants[resourceType][id] ?? [];
        } catch {
            return [];
        }
    },

    async fetchIncomingGrants() {
        const response = await fetch('/api/grants/incoming');

        if (!response.ok) {
            console.log(`error ${response.status} while fetching /api/grants/incoming:`, await response.json);
            return;
        }

        /** @type {Grant[]} */
        const incomingGrants = await response.json();

        // store grants by type, then by id
        incomingGrants.forEach((grant) => {
            this.incomingGrants[grant.resource.type] ??= {};
            this.incomingGrants[grant.resource.type][grant.resource.id] ??= [];
            this.incomingGrants[grant.resource.type][grant.resource.id].push(grant);
        });

        console.log(`incoming grants: `, this.incomingGrants);
    },

    /**
     * get grant for a resource
     * @param {ResourceTypeEnum} resourceType
     * @param {String} id
     * @returns {Grant[] | null}
     */
    getIncomingGrantsFor(resourceType, id) {
        try {
            return this.incomingGrants[resourceType][id] ?? [];
        } catch {
            return [];
        }
    },

    /**
     * Fetch a cursor-paginated list of resources shared with the current user,
     * with full file / folder metadata resolved server-side.
     *
     * @param {object}            [opts]
     * @param {ResourceTypeEnum[]} [opts.resourceTypes] - Resource types to include (default: ['file','folder']).
     * @param {number}             [opts.limit]         - Max items per page (1–200, default 50).
     * @param {string}             [opts.cursor]        - Opaque cursor from a previous call; omit for first page.
     * @returns {Promise<SharedWithMeResponse>}
     */
    async fetchSharedWithMe({ resourceTypes = ['file', 'folder'], limit = 50, cursor } = {}) {
        const params = new URLSearchParams({
            limit: String(limit),
            resource_types: resourceTypes.join(',')
        });
        if (cursor) params.set('cursor', cursor);

        const response = await fetch(`/api/grants/incoming/resources?${params}`);

        if (!response.ok) {
            throw new Error(`Failed to fetch shared-with-me items: HTTP ${response.status}`);
        }

        return response.json();
    }
};

export { grants };
