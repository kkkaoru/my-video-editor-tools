// Runs with bun: hands the active language to every view.
import { createContext } from "preact";
import { useContext } from "preact/hooks";
import { enMessages } from "./locale-en.ts";
import type { Messages } from "./messages.ts";

export const MessagesContext = createContext<Messages>(enMessages);

export const useMessages = (): Messages => useContext(MessagesContext);
