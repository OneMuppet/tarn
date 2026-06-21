export interface User {
    id: number;
}

export type Id = number;

export class Repo {
    find(id: Id): User {
        return load(id);
    }
}

export function load(id: Id): User {
    return { id };
}
