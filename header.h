#ifndef HEADER_H
#define HEADER_H

#include <iostream>
#include <string>
#include <vector>

using namespace std;

// Configuración: Máximo tamaño de una hoja antes de dividirla (opcional por ahora)
const int LEAF_MAX = 8; 

struct RopeNode {
    RopeNode* left;
    RopeNode* right;
    std::string data; // Solo tendrá datos si es una hoja
    int weight;       // El peso mágico

    // Constructor para Hoja (Leaf)
    RopeNode(std::string str) : left(nullptr), right(nullptr), data(str) {
        weight = str.length(); 
        // Nota: En muchas implementaciones, el weight de una hoja es su longitud,
        // pero en nodos internos es la suma del peso de todo el subárbol IZQUIERDO.
        // Para simplificar este ejemplo inicial, usaremos weight = longitud del nodo actual o subarbol izq.
    }

    // Constructor para Nodo Interno (Concatenación)
    RopeNode(RopeNode* l, RopeNode* r) : left(l), right(r), data("") {
        // El peso de un nodo interno es la longitud total de su lado IZQUIERDO
        weight = l->total_length(); 
    }
    
    // Función auxiliar para saber el largo total debajo de este nodo
    int total_length() {
        if (!left && !right) return weight; // Es hoja
        // Si es interno, sumamos el peso (lado izquierdo) + largo del lado derecho
        return weight + (right ? right->total_length() : 0);
    }
    
    // Destructor para limpiar memoria (básico)
    ~RopeNode() {
        if (left) delete left;
        if (right) delete right;
    }
};

char get_char(RopeNode* node, int index) {
    // 1. Caso base: ¿Es una hoja?
    if (node->left == nullptr && node->right == nullptr) {
        // Si el índice pedido está dentro de este string, lo devolvemos
        if (index < node->data.length()) {
            return node->data[index];
        } else {
            // Esto no debería pasar si la lógica externa está bien
            throw out_of_range("Índice fuera de rango en hoja");
        }
    }

    // 2. Decisión recursiva: ¿Vamos a la izquierda o derecha?
    if (index < node->weight) {
        // El carácter está en el lado izquierdo
        return get_char(node->left, index);
    } else {
        // El carácter está en el lado derecho
        // TRUCO IMPORTANTE: Restamos el peso porque ya "pasamos" esos caracteres
        return get_char(node->right, index - node->weight);
    }
}
RopeNode* concat(RopeNode* n1, RopeNode* n2) {
    // Simplemente creamos un nuevo padre
    return new RopeNode(n1, n2); 
}

#endif