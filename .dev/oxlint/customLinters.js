const PROVABLY_UNNECESSARY_FALLBACK_RULE = {
    meta: {
        type: 'problem',
        docs: {
            description: 'Detect fallback expressions that are provably unnecessary',
        },
        schema: [],
        messages: {
            unnecessaryNullish: 'Provably unnecessary fallback: left side of ?? is never null/undefined.',
            unnecessaryOr: 'Provably unnecessary fallback: left side of || is always truthy.',
        },
    },
    create(context) {
        function isDefinitelyNonNullish(node) {
            switch (node.type) {
                case 'Literal':
                    return node.value !== null;
                case 'ArrayExpression':
                case 'ObjectExpression':
                case 'FunctionExpression':
                case 'ArrowFunctionExpression':
                case 'ClassExpression':
                case 'NewExpression':
                    return true;
                case 'TemplateLiteral':
                    return true;
                default:
                    return false;
            }
        }

        function isDefinitelyTruthy(node) {
            switch (node.type) {
                case 'Literal':
                    if (typeof node.value === 'boolean') {
                        return node.value;
                    }

                    if (typeof node.value === 'number') {
                        return node.value !== 0;
                    }

                    if (typeof node.value === 'bigint') {
                        return node.value !== 0n;
                    }

                    if (typeof node.value === 'string') {
                        return node.value.length > 0;
                    }

                    return node.regex != null;
                case 'ArrayExpression':
                case 'ObjectExpression':
                case 'FunctionExpression':
                case 'ArrowFunctionExpression':
                case 'ClassExpression':
                case 'NewExpression':
                    return true;
                default:
                    return false;
            }
        }

        return {
            LogicalExpression(node) {
                if (node.operator === '??' && isDefinitelyNonNullish(node.left)) {
                    context.report({ node, messageId: 'unnecessaryNullish' });
                }

                if (node.operator === '||' && isDefinitelyTruthy(node.left)) {
                    context.report({ node, messageId: 'unnecessaryOr' });
                }
            },
        };
    },
};

function isPromiseRejectCall(node) {
    if (!node || node.type !== 'CallExpression') {
        return false;
    }

    if (!node.callee || node.callee.type !== 'MemberExpression' || node.callee.computed) {
        return false;
    }

    return (
        node.callee.object.type === 'Identifier' &&
        node.callee.object.name === 'Promise' &&
        node.callee.property.type === 'Identifier' &&
        node.callee.property.name === 'reject'
    );
}

function isThrowLikeExpression(node) {
    if (!node) {
        return false;
    }

    if (node.type === 'AwaitExpression') {
        return isThrowLikeExpression(node.argument);
    }

    return isPromiseRejectCall(node);
}

function traverse(node, visitor) {
    if (!node || typeof node.type !== 'string') {
        return;
    }

    visitor(node);

    for (const value of Object.values(node)) {
        if (Array.isArray(value)) {
            for (const child of value) {
                if (child && typeof child.type === 'string') {
                    traverse(child, visitor);
                }
            }
            continue;
        }

        if (value && typeof value.type === 'string') {
            traverse(value, visitor);
        }
    }
}

function isFunctionNode(node) {
    return node.type === 'FunctionExpression' || node.type === 'ArrowFunctionExpression';
}

function callbackReturnsSuccessValue(callback) {
    if (!isFunctionNode(callback)) {
        return false;
    }

    if (callback.body.type !== 'BlockStatement') {
        return !isThrowLikeExpression(callback.body);
    }

    let foundSuccessReturn = false;
    traverse(callback.body, (node) => {
        if (isFunctionNode(node) && node !== callback) {
            return;
        }

        if (node.type === 'ReturnStatement' && node.argument && !isThrowLikeExpression(node.argument)) {
            foundSuccessReturn = true;
        }
    });

    return foundSuccessReturn;
}

function blockHasThrow(block) {
    let hasThrow = false;
    traverse(block, (node) => {
        if (node.type === 'ThrowStatement') {
            hasThrow = true;
        }
    });
    return hasThrow;
}

function catchReturnsSuccess(catchClause) {
    if (!catchClause || !catchClause.body || catchClause.body.type !== 'BlockStatement') {
        return [];
    }

    if (blockHasThrow(catchClause.body)) {
        return [];
    }

    const returns = [];
    traverse(catchClause.body, (node) => {
        if (node.type === 'ReturnStatement' && node.argument && !isThrowLikeExpression(node.argument)) {
            returns.push(node);
        }
    });

    return returns;
}

const SUSPICIOUS_FALLBACK_RULE = {
    meta: {
        type: 'problem',
        docs: {
            description: 'Detect suspicious fallback paths where a failure branch recovers to success',
        },
        schema: [],
        messages: {
            catchRecovery: 'Suspicious fallback: catch branch returns a success value.',
            promiseCatchRecovery: 'Suspicious fallback: .catch() callback returns a success value.',
        },
    },
    create(context) {
        return {
            TryStatement(node) {
                const successReturns = catchReturnsSuccess(node.handler);
                for (const returnNode of successReturns) {
                    context.report({ node: returnNode, messageId: 'catchRecovery' });
                }
            },
            CallExpression(node) {
                if (!node.callee || node.callee.type !== 'MemberExpression' || node.callee.computed) {
                    return;
                }

                if (node.callee.property.type !== 'Identifier' || node.callee.property.name !== 'catch') {
                    return;
                }

                const callback = node.arguments[0];
                if (callback && callbackReturnsSuccessValue(callback)) {
                    context.report({ node: callback, messageId: 'promiseCatchRecovery' });
                }
            },
        };
    },
};

export default {
    meta: {
        name: 'agentlinters',
    },
    rules: {
        'no-provably-unnecessary-fallback': PROVABLY_UNNECESSARY_FALLBACK_RULE,
        'no-suspicious-fallback': SUSPICIOUS_FALLBACK_RULE,
    },
};
