/** A `catch` handler that logs `${command} failed` with the error. */
export const failed = (command: string) => (e: unknown) => console.error(`${command} failed`, e)
