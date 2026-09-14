const STORAGE_KEY = 'rho_fleet_nodes_v1';

export const NodeRegistry = {
  getNodes() {
    try {
      const data = localStorage.getItem(STORAGE_KEY);
      return data ? JSON.parse(data) : [];
    } catch {
      return [];
    }
  },

  getNode(id) {
    const nodes = this.getNodes();
    return nodes.find((n) => n.id === id) || null;
  },

  saveNode(node) {
    const nodes = this.getNodes();
    const idx = nodes.findIndex((n) => n.id === node.id);
    if (idx >= 0) {
      nodes[idx] = { ...nodes[idx], ...node, lastSeen: Date.now() };
    } else {
      nodes.push({ ...node, lastSeen: Date.now() });
    }
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(nodes));
    } catch (e) {
      console.error('Failed to persist node registry', e);
    }
  },

  removeNode(id) {
    const nodes = this.getNodes().filter((n) => n.id !== id);
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(nodes));
    } catch (e) {
      console.error('Failed to remove node from registry', e);
    }
  },

  updateNodeInfo(id, info) {
    const node = this.getNode(id);
    if (node) {
      this.saveNode({ ...node, info });
    }
  }
};
