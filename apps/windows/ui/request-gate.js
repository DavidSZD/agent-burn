export class RequestGate {
  #generation = 0;

  begin() {
    this.#generation += 1;
    return this.#generation;
  }

  isCurrent(generation) {
    return generation === this.#generation;
  }
}
