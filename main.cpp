#include <iostream>
#include "header.h"

int main(){
    
    cout << "--- Inicio de prueba Rope ---"<< endl;

    RopeNode* hoja1 = new RopeNode("Hola ");
    RopeNode* hoja2 = new RopeNode("Mundo");

    RopeNode* root = concat(hoja1,hoja2);

    // D. Verificar estructura
    cout << "Arbol construido." << endl;
    cout << "Peso del nodo raiz (longitud lado izquierdo): " << root->weight << endl;
    cout << "Longitud total del texto: " << root->total_length() << endl;
    
    cout << "\nLectura caracter por caracter: \n[";

    int len = root->total_length();
    for (int i = 0; i < len; i++) {
        std::cout << get_char(root, i);
    }

    cout << "]" << std::endl;

    delete root;

    return 0;
}