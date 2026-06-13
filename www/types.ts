export interface Example {
  name: string;
  code: string | null;
  eventData?: string;
}

export interface LogEntry {
  time: string;
  message: string;
  type: 'info' | 'success' | 'error' | 'warn';
}

export interface EventLogEntry {
  time: string;
  workflow: string;
  logs: string[];
  data: Record<string, unknown>;
}

export type TabName = 'output' | 'ast' | 'events';

export const EXAMPLES: Example[] = [
  {
    name: 'Hello World',
    code: `workflow "Hello World" {\n  on GREET\n  log("Hello, World!")\n}`,
    eventData: `{}`
  },
  {
    name: 'With Import',
    code: `import data from "./schema.json"\n\nworkflow "User Greeting" {\n  on USER_LOGIN\n  log("Welcome, " + data.userId + "!")\n  log("Plan: " + data.plan)\n}`,
    eventData: `{"userId": "user123", "plan": "premium"}`
  },
  {
    name: 'Nested Loops',
    code: `workflow "Nested Loops" {\n  on NESTED_DATA ({users, meta})\n  log("Users: " + users.length + ", Meta: " + meta.length)\n  foreach (user in users) {\n    log("User: " + user.name)\n    foreach (order in user.orders) {\n      log("  Order: " + order.id)\n      if (order.total > 100) {\n        log("    High value order")\n      }\n    }\n  }\n}`,
    eventData: `{"users": [{"name": "Alice", "orders": [{"id": "ORD-001", "total": 250}, {"id": "ORD-002", "total": 75}]}, {"name": "Bob", "orders": [{"id": "ORD-003", "total": 500}]}], "meta": [{"key": "source", "value": "web"}]}`
  },
  {
    name: 'If/Else',
    code: `workflow "User Check" {\n  on USER_LOGIN\n  if (data.plan == "premium") {\n    log("Welcome back, premium user!")\n  } else {\n    log("Free tier user")\n  }\n}`,
    eventData: `{"userId": "user123", "plan": "premium"}`
  },
  {
    name: 'Functions',
    code: `fn double(x) {\n  return x * 2\n}\n\nfn greet(name) {\n  log("Hello, " + name + "!")\n}\n\nworkflow "With Functions" {\n  on CALCULATE\n  var result = double(data.value)\n  log("Doubled: " + result)\n  greet(data.name)\n}`,
    eventData: `{"value": 21, "name": "World"}`
  },
  {
    name: 'Batch Process',
    code: `workflow "Batch" {\n  on BATCH_START\n  var total = 0\n  foreach (item in data.items) {\n    log("Processing: " + item.name)\n    var total = total + item.amount\n  }\n  log("Total: " + total)\n}`,
    eventData: `{"items": [{"name": "Widget A", "amount": 25}, {"name": "Widget B", "amount": 50}, {"name": "Widget C", "amount": 15}]}`
  },
  {
    name: 'Complex Logic',
    code: `workflow "Fraud Check" {\n  on TRANSACTION\n  if (data.amount > 1000 && data.country != "US") {\n    log("ALERT: High value international transaction")\n    log("Amount: " + data.amount)\n  } else if (data.amount > 500) {\n    log("Review: Medium value transaction")\n  } else {\n    log("OK: Normal transaction")\n  }\n}`,
    eventData: `{"amount": 1500, "country": "UK", "userId": "user456"}`
  },
  {
    name: 'Nested Loops + JSON',
    code: null,
    eventData: `{"users": [{"name": "Alice", "orders": [{"id": "ORD-001", "total": 250}, {"id": "ORD-002", "total": 75}]}, {"name": "Bob", "orders": [{"id": "ORD-003", "total": 500}]}], "meta": [{"key": "source", "value": "web"}]}`
  },
  {
    name: 'Emit Workflows',
    code: `workflow "User Router" {\n  on USER_EVENT\n  log("Processing user: " + data.userId)\n  if (data.role == "admin") {\n    emit("Admin Handler", {userId: data.userId, action: "elevate"})\n  } else {\n    emit("User Handler", {userId: data.userId, action: "standard"})\n  }\n}\n\nworkflow "Admin Handler" {\n  on ADMIN_EVENT\n  log("Admin action for: " + data.userId)\n  log("Action: " + data.action)\n}\n\nworkflow "User Handler" {\n  on USER_ACTION\n  log("Standard user: " + data.userId)\n  log("Action: " + data.action)\n}`,
    eventData: `{"userId": "user123", "role": "admin"}`
  }
];
