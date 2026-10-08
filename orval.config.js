module.exports = {
  mairie360: {
    input: "./openapi.json",
    output: {
      mode: "split", // C'EST LA CLÉ : un fichier par Tag (Roles, Sessions, etc.)
      target: "generated/endpoints",
      schemas: "generated/model", // Met tous les types/interfaces ici
      client: "axios", // ou 'fetch', 'axios' selon ton besoin
      mock: false,
    },
  },
};
